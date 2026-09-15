//! Reaction role bot: react to a message, get a role; un-react, lose it.
//!
//! This is split into two parts, the Core, which is purely functional,
//! and the Shell, which handles IO (not functional)

use serenity::all::*;
use serenity::async_trait;

// Core

/// "React with `emoji` on `message` and you get `role`."
/// The channel is only there because Discord needs it to place a reaction.
#[derive(Clone, Debug, PartialEq)]
struct Rule {
    channel: ChannelId,
    message: MessageId,
    emoji: ReactionType,
    role: RoleId,
}

/// One rule per line: `<channel_id> <message_id> <emoji> <role_id>`.
/// Blank lines and lines starting with `#` are ignored, as is any line that
/// does not parse (a typo silently drops one rule instead of killing the bot).
fn parse_rules(src: &str) -> Vec<Rule> {
    src.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| match line.split_whitespace().collect::<Vec<_>>()[..] {
            [channel, message, emoji, role] => Some(Rule {
                channel: ChannelId::new(id(channel)?),
                message: MessageId::new(id(message)?),
                emoji: ReactionType::try_from(emoji).ok()?,
                role: RoleId::new(id(role)?),
            }),
            _ => None,
        })
        .collect()
}

/// A Discord snowflake: a non-zero u64, the user id
fn id(text: &str) -> Option<u64> {
    text.parse().ok().filter(|&n| n != 0)
}

/// Two reactions are "the same" when this matches: the character itself for a
/// standard emoji, the id for a custom one (whose name can change under us).
fn emoji_key(emoji: &ReactionType) -> String {
    match emoji {
        ReactionType::Custom { id, .. } => id.to_string(),
        other => other.to_string(),
    }
}

/// The role a reaction gets, if any.
fn role_for(rules: &[Rule], message: MessageId, emoji: &ReactionType) -> Option<RoleId> {
    let key = emoji_key(emoji);
    rules
        .iter()
        .find(|rule| rule.message == message && emoji_key(&rule.emoji) == key)
        .map(|rule| rule.role)
}

/// Every message the rules mention, once each.
fn targets(rules: &[Rule]) -> Vec<(ChannelId, MessageId)> {
    let mut pairs: Vec<_> = rules.iter().map(|rule| (rule.channel, rule.message)).collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// Of the reactions sitting on `message`, the ones no rule allows.
fn strays(rules: &[Rule], message: MessageId, present: &[ReactionType]) -> Vec<ReactionType> {
    present
        .iter()
        .filter(|emoji| role_for(rules, message, emoji).is_none())
        .cloned()
        .collect()
}

// SHELL

/// How often to sweep off reactions that earn no role.
// ponytail: fixed interval, no command to run it by hand. Add one if waiting
// an hour for a stray to disappear ever gets annoying.
const SWEEP: std::time::Duration = std::time::Duration::from_secs(60 * 60);

struct Handler {
    rules: Vec<Rule>,
    /// Set once the sweeper is running, so a reconnect does not start a second.
    sweeping: std::sync::OnceLock<()>,
}

/// Forever: every `SWEEP`, take off any reaction that no rule allows.
async fn sweep(http: std::sync::Arc<Http>, rules: Vec<Rule>) {
    let mut clock = tokio::time::interval(SWEEP);
    loop {
        clock.tick().await;
        for (channel, message) in targets(&rules) {
            let posted = match http.get_message(channel, message).await {
                Ok(posted) => posted,
                Err(why) => {
                    eprintln!("could not read message {message}: {why}");
                    continue;
                },
            };
            let present: Vec<_> =
                posted.reactions.into_iter().map(|found| found.reaction_type).collect();
            for stray in strays(&rules, message, &present) {
                let cleared = http.delete_message_reaction_emoji(channel, message, &stray).await;
                if let Err(why) = cleared {
                    eprintln!("could not clear {stray} from message {message}: {why}");
                }
            }
        }
    }
}

impl Handler {
    async fn apply(&self, ctx: &Context, reaction: &Reaction, grant: bool) {
        let (Some(guild), Some(user)) = (reaction.guild_id, reaction.user_id) else {
            return; // a DM: no roles to hand out
        };
        if reaction.member.as_ref().is_some_and(|member| member.user.bot) {
            return; // our own startup reactions, and any other bot's
        }
        let Some(role) = role_for(&self.rules, reaction.message_id, &reaction.emoji) else {
            return; // not a reaction we care about
        };
        let reason = Some("reaction role");
        let result = if grant {
            ctx.http.add_member_role(guild, user, role, reason).await
        } else {
            ctx.http.remove_member_role(guild, user, role, reason).await
        };
        if let Err(why) = result {
            eprintln!("role {role} for user {user}: {why}");
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn reaction_add(&self, ctx: Context, reaction: Reaction) {
        self.apply(&ctx, &reaction, true).await;
    }

    async fn reaction_remove(&self, ctx: Context, reaction: Reaction) {
        self.apply(&ctx, &reaction, false).await;
    }

    /// On connect, put every rule's emoji on its message so members have
    /// something to click.
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} online with {} rules", ready.user.name, self.rules.len());
        for rule in &self.rules {
            let placed = ctx.http.create_reaction(rule.channel, rule.message, &rule.emoji).await;
            if let Err(why) = placed {
                eprintln!("could not react with {} on message {}: {why}", rule.emoji, rule.message);
            }
        }

        if self.sweeping.set(()).is_ok() {
            tokio::spawn(sweep(ctx.http.clone(), self.rules.clone()));
        }
    }
}

/// Where the token is kept between runs. Plain text, so keep it out of git.
const TOKEN_FILE: &str = "token.txt";

/// The bot token: whatever is in `token.txt`, or ask for it once and save it.
fn load_token() -> String {
    if let Ok(saved) = std::fs::read_to_string(TOKEN_FILE)
        && !saved.trim().is_empty()
    {
        return saved.trim().to_string();
    }

    println!("Paste your bot token, then press Enter:");
    let mut typed = String::new();
    std::io::stdin().read_line(&mut typed).expect("could not read from the console");

    let token = typed.trim().to_string();
    assert!(!token.is_empty(), "no token entered");
    std::fs::write(TOKEN_FILE, &token).expect("could not write token.txt");
    println!("Saved to {TOKEN_FILE}. You will not be asked again.");
    token
}

#[tokio::main]
async fn main() {
    let token = load_token();
    let file = std::fs::read_to_string("roles.txt").expect("roles.txt not found");
    let handler = Handler { rules: parse_rules(&file), sweeping: std::sync::OnceLock::new() };

    Client::builder(&token, GatewayIntents::GUILD_MESSAGE_REACTIONS)
        .event_handler(handler)
        .await
        .expect("could not build client")
        .start()
        .await
        .expect("gateway stopped (bad token? delete token.txt to enter it again)");
}

// TESTS

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "
        # pick a language
        10 111 🦀 222
        10 111 <:haskell:987654321> 333
        garbage line
        10 111 🦀 0
    ";

    #[test]
    fn parsing_keeps_only_well_formed_lines() {
        let rules = parse_rules(RULES);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0], Rule {
            channel: ChannelId::new(10),
            message: MessageId::new(111),
            emoji: ReactionType::Unicode("🦀".into()),
            role: RoleId::new(222),
        });
    }

    /// Every non-comment line of the real roles.txt survives parsing — a typo
    /// there silently drops a role, which is the failure you would not notice.
    #[test]
    fn shipped_roles_file_parses() {
        let file = std::fs::read_to_string("roles.txt").expect("roles.txt");
        let lines = file
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .count();
        assert_eq!(parse_rules(&file).len(), lines);
    }

    #[test]
    fn sweep_spares_only_the_listed_reactions() {
        let rules = parse_rules(RULES);
        let crab = ReactionType::Unicode("🦀".into());
        let snake = ReactionType::Unicode("🐍".into());
        let present = [crab.clone(), snake.clone()];

        assert_eq!(targets(&rules), vec![(ChannelId::new(10), MessageId::new(111))]);
        assert_eq!(strays(&rules, MessageId::new(111), &present), vec![snake]);
        // A message no rule mentions keeps nothing.
        assert_eq!(strays(&rules, MessageId::new(999), &present), present.to_vec());
    }

    #[test]
    fn lookup_matches_message_and_emoji() {
        let rules = parse_rules(RULES);
        let rus = ReactionType::Unicode("🦀".into());
        let pyt = ReactionType::Unicode("🐍".into());
        let custom = ReactionType::Custom {
            animated: false,
            id: EmojiId::new(987654321),
            name: Some("haskell".into()),
        };

        assert_eq!(role_for(&rules, MessageId::new(111), &rus), Some(RoleId::new(222)));
        assert_eq!(role_for(&rules, MessageId::new(111), &custom), Some(RoleId::new(333)));
        assert_eq!(role_for(&rules, MessageId::new(111), &pyt), None);
        assert_eq!(role_for(&rules, MessageId::new(999), &rus), None);
    }
}
