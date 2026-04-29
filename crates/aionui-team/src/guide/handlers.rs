use serde_json::Value;

#[derive(Debug, Clone)]
pub struct CreateTeamParams {
    pub summary: String,
    pub name: String,
    pub workspace: String,
}

/// Parse `aion_create_team` tool arguments into structured params.
///
/// Defaults:
/// - `name` falls back to the first 5 whitespace-separated tokens of `summary`.
/// - `workspace` falls back to the caller's workspace, then to `"."`.
pub fn parse_create_team_args(args: &Value, caller_workspace: Option<&str>) -> Result<CreateTeamParams, String> {
    let summary = args
        .get("summary")
        .and_then(Value::as_str)
        .ok_or("missing required field: summary")?
        .to_owned();

    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| summary.split_whitespace().take(5).collect::<Vec<_>>().join(" "));

    let workspace = args
        .get("workspace")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| caller_workspace.map(String::from))
        .unwrap_or_else(|| ".".to_owned());

    Ok(CreateTeamParams {
        summary,
        name,
        workspace,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn errors_when_summary_missing() {
        let args = json!({ "name": "alpha", "workspace": "/tmp" });
        let err = parse_create_team_args(&args, None).unwrap_err();
        assert!(err.contains("summary"), "unexpected error: {err}");
    }

    #[test]
    fn errors_when_summary_not_string() {
        let args = json!({ "summary": 42 });
        let err = parse_create_team_args(&args, None).unwrap_err();
        assert!(err.contains("summary"), "unexpected error: {err}");
    }

    #[test]
    fn name_defaults_to_first_five_summary_words() {
        let args = json!({
            "summary": "implement login flow and add OAuth provider support end-to-end",
        });
        let params = parse_create_team_args(&args, None).unwrap();
        assert_eq!(params.name, "implement login flow and add");
        assert_eq!(
            params.summary,
            "implement login flow and add OAuth provider support end-to-end"
        );
    }

    #[test]
    fn name_defaults_use_all_summary_when_shorter_than_five_words() {
        let args = json!({ "summary": "hello world" });
        let params = parse_create_team_args(&args, None).unwrap();
        assert_eq!(params.name, "hello world");
    }

    #[test]
    fn workspace_inherits_from_caller_when_missing() {
        let args = json!({ "summary": "do work" });
        let params = parse_create_team_args(&args, Some("/caller/ws")).unwrap();
        assert_eq!(params.workspace, "/caller/ws");
    }

    #[test]
    fn workspace_defaults_to_dot_when_caller_absent() {
        let args = json!({ "summary": "do work" });
        let params = parse_create_team_args(&args, None).unwrap();
        assert_eq!(params.workspace, ".");
    }

    #[test]
    fn custom_fields_take_precedence_over_defaults() {
        let args = json!({
            "summary": "refactor the scheduler end-to-end",
            "name": "scheduler-refactor",
            "workspace": "/repo/path",
        });
        let params = parse_create_team_args(&args, Some("/caller/ws")).unwrap();
        assert_eq!(params.summary, "refactor the scheduler end-to-end");
        assert_eq!(params.name, "scheduler-refactor");
        assert_eq!(params.workspace, "/repo/path");
    }

    #[test]
    fn non_string_name_falls_back_to_summary_prefix() {
        let args = json!({
            "summary": "one two three four five six",
            "name": 123,
        });
        let params = parse_create_team_args(&args, None).unwrap();
        assert_eq!(params.name, "one two three four five");
    }

    #[test]
    fn non_string_workspace_falls_back_to_caller() {
        let args = json!({
            "summary": "do work",
            "workspace": 42,
        });
        let params = parse_create_team_args(&args, Some("/caller/ws")).unwrap();
        assert_eq!(params.workspace, "/caller/ws");
    }
}
