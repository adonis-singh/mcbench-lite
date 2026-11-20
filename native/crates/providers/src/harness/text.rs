//! Text that reaches models through the CLI harnesses, as today's sources
//! hold it. Each piece is the literal text of a template in `server/`, with
//! its holes marked; `pieces_are_in_todays_sources` (always run) finds every
//! piece, in its JavaScript source form, in the file it names, and the
//! golden and differential tests compare whole prompts with Node's.

/// `harness.js` 23-26 `dataImage`.
pub(crate) const BAD_IMAGE: &str = "Harness images must be base64 data URLs";
/// `harness.js` 43 and 128.
pub(crate) const UNSUPPORTED: (&str, &str) = ("Unsupported harness content type: ", "");
/// `harness.js` 41: an image's placeholder, `[Image N]`.
pub(crate) const IMAGE: (&str, &str) = ("[Image ", "]");
/// `harness.js` 62: a user message of the transcript.
pub(crate) const USER: &str = "User:\n";
/// `harness.js` 63: a tool result, `Result of <name> (<id>):\n<content>`.
pub(crate) const RESULT: (&str, &str, &str) = ("Result of ", " (", "):\n");
/// `harness.js` 66: a call, `Called <name> (<id>) with <arguments>`.
pub(crate) const CALLED: (&str, &str, &str) = ("Called ", " (", ") with ");
/// `harness.js` 66: arguments a call carried none of.
pub(crate) const NO_ARGUMENTS: &str = "{}";
/// `harness.js` 68: an assistant message, with `\n<content>` when it has some.
pub(crate) const ASSISTANT: &str = "Assistant:";
/// `harness.js` 82: the earlier transcript around its entries.
pub(crate) const CONVERSATION: (&str, &str) = ("<conversation>\n", "\n</conversation>");
/// `harness.js` 84: the request when there is no new user message.
pub(crate) const CONTINUE: &str = "Continue the current task from the latest tool results above.";
/// `harness.js` 117 `stoppedReplyNote`, around the reply the user saw.
pub(crate) const STOPPED_REPLY: (&str, &str) = (
    "<stopped-reply>\nThe user stopped your previous reply while you were writing it, and this session does not hold it. This is the text you had written, as the user saw it:\n\nAssistant:\n",
    "\n</stopped-reply>",
);
/// `prompt.js` 16 `RESOLUTION_HEADING`.
pub(crate) const RESOLUTION_HEADING: &str = "## Resolution";
/// `prompt.js` 44 `resolutionUpdate`, around the current resolution section.
pub(crate) const RESOLUTION_CHANGE: (&str, &str) = (
    "<resolution-change>\nThe chat's resolution has changed. From this request on, this section replaces the Resolution section of your instructions and any earlier resolution change:\n\n",
    "\n</resolution-change>",
);
/// `prompt.js` 45 `resolutionUpdate`, back at Standard (its JavaScript
/// source escapes the apostrophe: `chat\'s`).
pub(crate) const RESOLUTION_STANDARD: &str = "<resolution-change>\nThe chat's resolution has changed to Standard. From this request on, the Resolution section of your instructions and any earlier resolution change no longer apply: build at the scale and detail you would by default.\n</resolution-change>";
/// `harness.js` 272, 363 and 439: a call the turn will not run.
pub(crate) const TURN_ENDED: &str = "The mcbench turn ended before this tool ran.";
/// `harness.js` 355: `<name> ended with a tool call unanswered`.
pub(crate) const UNANSWERED: &str = " ended with a tool call unanswered";
/// `harness.js` 261: `<name> run closed`.
pub(crate) const RUN_CLOSED: &str = " run closed";
/// The reason of `AbortController.abort()` with none given (a DOMException),
/// which a stopped turn's completion rejects with today (`index.js` 1294).
pub(crate) const ABORTED: &str = "This operation was aborted";
/// `claude-code.js` 245.
pub(crate) const NO_TOOL_USE_ID: &str =
    "Claude Code called an mcbench tool without its tool_use id";
/// `claude-code.js` 264.
pub(crate) const NO_TURN: &str = "No mcbench turn is running in this Claude Code process";
/// `claude-code.js` 267.
pub(crate) const STOPPED_WHILE_STARTING: &str = "Claude Code was stopped while starting";
/// `claude-code.js` 130.
pub(crate) const OUTSIDE_MCBENCH: &str = "Claude Code exposed tools outside mcbench: ";
/// `claude-code.js` 132.
pub(crate) const SERVER_STATUS: &str = "mcbench MCP server status: ";
/// `claude-code.js` 151.
pub(crate) const COMPACTED: &str =
    "Claude Code compacted the conversation to fit its context window";
/// `claude-code.js` 149.
pub(crate) const COMPACTING: &str = "Compacting context";
/// `claude-code.js` 150: `Retrying[ after HTTP <status>] (attempt <n> of <max>)`.
pub(crate) const RETRYING: (&str, &str, &str, &str, &str) =
    ("Retrying", " after HTTP ", " (attempt ", " of ", ")");
/// `codex-cli.js` 227: `Codex used a tool outside mcbench: <type>[ <server>]`.
pub(crate) const CODEX_OUTSIDE: &str = "Codex used a tool outside mcbench: ";
/// `codex-cli.js` 420.
pub(crate) const CODEX_FAILED: &str = "Codex turn failed: ";

#[cfg(test)]
mod tests {
    use super::*;

    /// JavaScript source form of a piece: what a `'...'` or template
    /// literal holds for it (`\n` written as backslash-n).
    fn source(text: &str) -> String {
        text.replace('\n', "\\n")
    }

    #[test]
    fn pieces_are_in_todays_sources() {
        let harness = include_str!("../../../../../server/harness.js");
        let prompt = include_str!("../../../../../server/prompt.js");
        let claude = include_str!("../../../../../server/claude-code.js");
        let codex = include_str!("../../../../../server/codex-cli.js");
        let found = |file: &str, piece: &str| {
            assert!(
                file.contains(&source(piece)),
                "{piece:?} is not in today's source"
            )
        };
        for piece in [
            BAD_IMAGE,
            UNSUPPORTED.0,
            IMAGE.0,
            USER,
            RESULT.0,
            CALLED.0,
            CALLED.2,
            NO_ARGUMENTS,
            ASSISTANT,
            CONVERSATION.0,
            CONVERSATION.1,
            CONTINUE,
            STOPPED_REPLY.0,
            STOPPED_REPLY.1,
            TURN_ENDED,
            UNANSWERED,
            RUN_CLOSED,
        ] {
            found(harness, piece);
        }
        found(prompt, RESOLUTION_HEADING);
        found(prompt, RESOLUTION_CHANGE.0);
        found(prompt, RESOLUTION_CHANGE.1);
        found(prompt, &RESOLUTION_STANDARD.replace("chat's", "chat\\'s"));
        for piece in [
            NO_TOOL_USE_ID,
            NO_TURN,
            STOPPED_WHILE_STARTING,
            OUTSIDE_MCBENCH,
            SERVER_STATUS,
            COMPACTED,
            COMPACTING,
            RETRYING.0,
            RETRYING.1,
            RETRYING.2,
            RETRYING.3,
        ] {
            found(claude, piece);
        }
        found(codex, CODEX_OUTSIDE);
        found(codex, CODEX_FAILED);
    }
}
