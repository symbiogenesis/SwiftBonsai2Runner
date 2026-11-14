//! CMake token collection without expansion or execution.

use crate::limit;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn bracket(text: &str, comment: bool) -> Option<(usize, String)> {
    let opening = if comment { "#[" } else { "[" };
    let rest = text.strip_prefix(opening)?;
    let equals = rest.bytes().take_while(|b| *b == b'=').count();
    if rest.as_bytes().get(equals) != Some(&b'[') {
        return None;
    }
    Some((
        opening.len() + equals + 1,
        format!("]{}]", "=".repeat(equals)),
    ))
}
fn skip_bracket(text: &str, prefix: usize, close: &str) -> Result<usize> {
    Ok(prefix
        + text[prefix..]
            .find(close)
            .ok_or("unterminated CMake bracket")?
        + close.len())
}

pub(crate) fn statements(text: &str) -> Result<Vec<Value>> {
    let (mut cursor, mut result) = (0, Vec::new());
    while cursor < text.len() {
        let tail = &text[cursor..];
        let ch = tail.chars().next().ok_or("missing CMake character")?;
        if ch.is_whitespace() {
            cursor += ch.len_utf8();
            continue;
        }
        if ch == '#' {
            cursor += if let Some((prefix, close)) = bracket(tail, true) {
                skip_bracket(tail, prefix, &close)?
            } else {
                tail.find('\n').unwrap_or(tail.len())
            };
            continue;
        }
        let start = cursor;
        let name_end = tail
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if name_end == 0 || tail.as_bytes()[0].is_ascii_digit() {
            return Err("unsupported CMake statement boundary".into());
        }
        let name = tail[..name_end].to_lowercase();
        cursor += name_end;
        while text[cursor..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            cursor += text[cursor..]
                .chars()
                .next()
                .ok_or("missing CMake character")?
                .len_utf8();
        }
        if text.as_bytes().get(cursor) != Some(&b'(') {
            return Err("unsupported CMake statement boundary".into());
        }
        cursor += 1;
        let (mut depth, mut quoted, mut token, mut tokens) =
            (1_i32, false, String::new(), Vec::new());
        while cursor < text.len() && depth > 0 {
            let tail = &text[cursor..];
            let ch = tail.chars().next().ok_or("missing CMake argument")?;
            if ch == '\\' && tail.len() > 1 {
                let end = 1 + tail[1..].chars().next().ok_or("missing escape")?.len_utf8();
                token.push_str(&tail[..end]);
                cursor += end;
                continue;
            }
            if ch == '"' {
                quoted = !quoted;
                token.push(ch);
                cursor += 1;
                continue;
            }
            if !quoted && ch == '#' {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                cursor += if let Some((prefix, close)) = bracket(tail, true) {
                    skip_bracket(tail, prefix, &close)?
                } else {
                    tail.find('\n').unwrap_or(tail.len())
                };
                continue;
            }
            if !quoted && ch == '[' {
                if let Some((prefix, close)) = bracket(tail, false) {
                    let extent = skip_bracket(tail, prefix, &close)?;
                    token.push_str(&tail[..extent]);
                    cursor += extent;
                    continue;
                }
            }
            if !quoted && (ch == '(' || ch == ')') {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
                depth += if ch == '(' { 1 } else { -1 };
                if depth != 0 {
                    tokens.push(ch.to_string());
                }
            } else if !quoted && ch.is_whitespace() {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            } else {
                token.push(ch);
            }
            cursor += ch.len_utf8();
        }
        if depth != 0 || quoted {
            return Err("unterminated CMake command".into());
        }
        result.push(json!({"command":name,"tokens":tokens,"startLine":text[..start].matches('\n').count()+1,"endLine":text[..cursor].matches('\n').count()+1}));
    }
    Ok(result)
}

pub(crate) fn metadata(recipes: &BTreeMap<String, Vec<u8>>, pin: &Value) -> Result<Value> {
    let mut definitions: BTreeMap<String, Vec<(String, Vec<Value>)>> = BTreeMap::new();
    let (mut outside, mut failures) = (Vec::new(), Vec::new());
    for (path, raw) in recipes {
        let parsed = std::str::from_utf8(raw)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                statements(text.trim_start_matches('\u{feff}')).map_err(|error| error.to_string())
            });
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(_) => {
                failures.push(path);
                continue;
            }
        };
        let mut cursor = 0;
        while cursor < parsed.len() {
            let statement = &parsed[cursor];
            if statement["command"] != "function" && statement["command"] != "macro" {
                outside.push((path, statement.clone()));
                cursor += 1;
                continue;
            }
            let (mut end, mut depth) = (cursor + 1, 1);
            while end < parsed.len() && depth > 0 {
                match parsed[end]["command"].as_str() {
                    Some("function" | "macro") => depth += 1,
                    Some("endfunction" | "endmacro") => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            let Some(name) = statement["tokens"][0].as_str().filter(|_| depth == 0) else {
                failures.push(path);
                break;
            };
            definitions
                .entry(name.to_lowercase())
                .or_default()
                .push((path.clone(), parsed[cursor..end].to_vec()));
            cursor = end;
        }
    }
    let mut pending = VecDeque::from(["build_idl".to_owned()]);
    let mut seen = BTreeSet::new();
    let mut chosen = Vec::new();
    let mut variables: BTreeSet<String> = ["proj-qaic_src", "HEXAGON_SDK_ROOT", "cmdLineOptions"]
        .map(str::to_owned)
        .into();
    while let Some(name) = pending.pop_front() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(matches) = definitions.get(&name).filter(|values| {
            values.len() == 1
                && chosen.len()
                    < usize::try_from(limit(pin, "recipeFunctions").unwrap_or(0)).unwrap_or(0)
        }) else {
            return Err("required CMake function absent, ambiguous or exceeds bound".into());
        };
        let (path, statements) = &matches[0];
        let mut emitted = statements.clone();
        for statement in &mut emitted {
            if statement["command"] == "message" {
                statement["tokens"] = json!([]);
                statement["nonfunctionalMessageTextOmitted"] = json!(true);
            } else if let Some(tokens) = statement["tokens"].as_array_mut() {
                for index in 0..tokens.len().saturating_sub(1) {
                    if tokens[index]
                        .as_str()
                        .is_some_and(|token| token.eq_ignore_ascii_case("COMMENT"))
                    {
                        tokens[index + 1] = json!("<nonfunctional-comment-text-omitted>");
                    }
                }
            }
        }
        chosen.push(json!({"name":name,"sourcePath":path,"statements":emitted}));
        for statement in statements {
            let command = statement["command"].as_str().ok_or("missing command")?;
            if definitions.contains_key(command) && !seen.contains(command) {
                pending.push_back(command.to_owned());
            }
            for token in statement["tokens"]
                .as_array()
                .ok_or("missing arguments")?
                .iter()
                .filter_map(Value::as_str)
            {
                let mut rest = token;
                while let Some(start) = rest.find('$') {
                    rest = &rest[start + 1..];
                    let candidate = rest.strip_prefix("ENV").unwrap_or(rest);
                    if let Some(candidate) = candidate.strip_prefix('{') {
                        if let Some(end) = candidate.find('}') {
                            variables.insert(candidate[..end].to_owned());
                        }
                    }
                }
            }
        }
    }
    let mut setup = Vec::new();
    for (path, mut statement) in outside {
        if !["set", "list", "include", "get_filename_component"]
            .contains(&statement["command"].as_str().unwrap_or_default())
        {
            continue;
        }
        let selected = statement["tokens"]
            .as_array()
            .ok_or("missing setup arguments")?
            .iter()
            .filter_map(Value::as_str)
            .any(|token| {
                variables.contains(token)
                    || ["qaic", "include", "/incs", "cmdlineoptions"]
                        .iter()
                        .any(|part| token.to_lowercase().contains(part))
            });
        if selected {
            statement["sourcePath"] = json!(path);
            setup.push(statement);
        }
    }
    let all: Vec<_> = chosen
        .iter()
        .flat_map(|function| function["statements"].as_array().into_iter().flatten())
        .chain(&setup)
        .collect();
    if all.len() > usize::try_from(limit(pin, "recipeStatements")?)? {
        return Err("selected functional CMake statement metadata exceeds bound".into());
    }
    for statement in all {
        let tokens = statement["tokens"]
            .as_array()
            .ok_or("missing recipe tokens")?;
        if tokens.len() > usize::try_from(limit(pin, "recipeStatementTokens")?)?
            || tokens.iter().any(|token| {
                token.as_str().is_none_or(|text| {
                    u64::try_from(text.encode_utf16().count()).unwrap_or(u64::MAX)
                        > limit(pin, "recipeTokenCharacters").unwrap_or(0)
                })
            })
        {
            return Err("selected CMake argument token metadata exceeds bound".into());
        }
    }
    Ok(
        json!({"status":"functional-token-metadata-collected-unreviewed","scope":"functional-command-token-metadata-only-no-comments-or-CMake-evaluation","functions":chosen,"relatedTopLevelSetup":setup,"otherRecipeFilesNotParsed":failures,"variableExpansionExecuted":false,"generatorCommandAuthorized":false}),
    )
}
