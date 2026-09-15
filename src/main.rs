//! Reaction role bot: react to a message, get a role; un-react, lose it.
//!
//! This is split into two parts, the Core, which is purely functional,
//! and the Shell, which handles IO (not functional)

use serenity::all::*;
use serenity::async_trait;

// Core

/// "React with `emoji` on `message` and you get `role`."
#[derive(Debug, PartialEq)]
struct Rule {
    message: MessageId,
    emoji: String,
    role: RoleId,
}

/// One rule per line: `<message_id> <emoji> <role_id>`.
/// Blank lines and lines starting with `#` are ignored, as is any line that
/// does not parse (a typo silently drops one rule instead of killing the bot).
fn parse_rules(src: &str) -> Vec<Rule> {
    src.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| match line.split_whitespace().collect::<Vec<_>>()[..] {
            [message, emoji, role] => Some(Rule {
                message: MessageId::new(id(message)?),
                emoji: emoji.to_string(),
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

/// How a reaction is written in the rules file: the character itself for a
/// standard emoji, the numeric id for a custom server emoji.
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
        .find(|rule| rule.message == message && rule.emoji == key)
        .map(|rule| rule.role)
}

// SHELL

struct Handler {
    rules: Vec<Rule>,
}

impl Handler {
    async fn apply(&self, ctx: &Context, reaction: &Reaction, grant: bool) {
        let (Some(guild), Some(user)) = (reaction.guild_id, reaction.user_id) else {
            return; // a DM: no roles to hand out
        };
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

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} online with {} rules", ready.user.name, self.rules.len());
    }
}

/// Where the token is kept between runs. Plain text, so keep it out of git.
const TOKEN_FILE: &str = "token.txt";

/// The bot token: whatever is in `token.txt`, or ask for it once and save it.
fn load_token() -> String {
    if let Ok(saved) = std::fs::read_to_string(TOKEN_FILE) {
        if !saved.trim().is_empty() {
            return saved.trim().to_string();
        }
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
    let handler = Handler { rules: parse_rules(&file) };

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
        111 🦀 222
        111 987654321 333
        garbage line
        111 🦀 0
    ";

    #[test]
    fn parsing_keeps_only_well_formed_lines() {
        let rules = parse_rules(RULES);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0], Rule {
            message: MessageId::new(111),
            emoji: "🦀".into(),
            role: RoleId::new(222),
        });
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
