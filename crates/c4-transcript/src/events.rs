// <wtl> project: c4 | file: crates/c4-transcript/src/events.rs | author: watchthelight </wtl>
//! From a record to the events the state machine reads. Most records produce nothing: thinking,
//! text, attachments and metadata do not move a session's state.

use crate::record::Record;
use c4_core::TranscriptEvent;

/// The events one record stands for, in the order to apply them.
#[must_use]
pub fn events_for(record: &Record) -> Vec<TranscriptEvent> {
    let mut out = Vec::new();
    match record.kind.as_str() {
        "user" => {
            if record.is_user_prompt() {
                out.push(TranscriptEvent::UserPrompt);
            }
            for block in record.tool_results() {
                out.push(TranscriptEvent::ToolResult {
                    tool_use_id: block.tool_use_id.clone().unwrap_or_default(),
                    is_error: block.is_error.unwrap_or(false),
                });
            }
            if let Some((agent_id, is_async)) = record.launched_agent() {
                out.push(TranscriptEvent::AgentLaunched { agent_id, is_async });
            }
        }
        "assistant" => {
            if record.is_api_error_message {
                let text = record
                    .message
                    .as_ref()
                    .map(|m| m.content.text())
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| "API error".to_string());
                out.push(TranscriptEvent::ApiError { message: text });
                return out;
            }
            for block in record.tool_uses() {
                out.push(TranscriptEvent::ToolUse {
                    tool_use_id: block.id.clone().unwrap_or_default(),
                    name: block.name.clone().unwrap_or_else(|| "tool".to_string()),
                });
            }
            if let Some(stop) = record.message.as_ref().and_then(|m| m.stop_reason.clone()) {
                out.push(TranscriptEvent::AssistantStopped {
                    stop_reason: Some(stop),
                });
            }
        }
        "system" => match record.subtype.as_deref() {
            Some("turn_duration") => out.push(TranscriptEvent::TurnEnded {
                pending_background_agents: record.pending_background_agent_count.unwrap_or(0),
                pending_workflows: record.pending_workflow_count.unwrap_or(0),
            }),
            Some("compact_boundary") => out.push(TranscriptEvent::CompactBoundary {
                trigger: record
                    .compact_metadata
                    .as_ref()
                    .and_then(|m| m.trigger.clone()),
            }),
            _ => {}
        },
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(line: &str) -> Record {
        Record::parse(line).unwrap()
    }

    #[test]
    fn prompts_tools_and_turns() {
        assert_eq!(
            events_for(&rec(
                r#"{"type":"user","message":{"role":"user","content":"hi"}}"#
            )),
            vec![TranscriptEvent::UserPrompt]
        );
        assert_eq!(
            events_for(&rec(
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash"}],"stop_reason":"tool_use"}}"#
            )),
            vec![
                TranscriptEvent::ToolUse {
                    tool_use_id: "t1".into(),
                    name: "Bash".into()
                },
                TranscriptEvent::AssistantStopped {
                    stop_reason: Some("tool_use".into())
                }
            ]
        );
        assert_eq!(
            events_for(&rec(
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":false}]},"toolUseResult":{"agentId":"ag","status":"async_launched","isAsync":true}}"#
            )),
            vec![
                TranscriptEvent::ToolResult {
                    tool_use_id: "t1".into(),
                    is_error: false
                },
                TranscriptEvent::AgentLaunched {
                    agent_id: "ag".into(),
                    is_async: true
                }
            ]
        );
        assert_eq!(
            events_for(&rec(
                r#"{"type":"system","subtype":"turn_duration","pendingBackgroundAgentCount":2}"#
            )),
            vec![TranscriptEvent::TurnEnded {
                pending_background_agents: 2,
                pending_workflows: 0
            }]
        );
        assert_eq!(
            events_for(&rec(
                r#"{"type":"system","subtype":"compact_boundary","compactMetadata":{"trigger":"auto"}}"#
            )),
            vec![TranscriptEvent::CompactBoundary {
                trigger: Some("auto".into())
            }]
        );
        assert_eq!(
            events_for(&rec(
                r#"{"type":"assistant","isApiErrorMessage":true,"message":{"content":[{"type":"text","text":"API Error: 529"}],"stop_reason":"stop_sequence"}}"#
            )),
            vec![TranscriptEvent::ApiError {
                message: "API Error: 529".into()
            }]
        );
    }

    #[test]
    fn silent_records() {
        for line in [
            r#"{"type":"ai-title","aiTitle":"x"}"#,
            r#"{"type":"cost-state","totalCostUSD":1}"#,
            r#"{"type":"attachment","attachment":{"type":"hook_success"}}"#,
            r#"{"type":"system","subtype":"stop_hook_summary"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"..."}],"stop_reason":null}}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"injected"}}"#,
        ] {
            assert_eq!(events_for(&rec(line)), vec![], "{line}");
        }
    }
}
