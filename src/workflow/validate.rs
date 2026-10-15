//! The checks a workflow passes before it runs. The first problem found is
//! reported, naming the file and the transition.
use super::actions::{self, Definition};
use super::format::{File, Transition, Trigger};
use super::{FAILED, WorkflowError, expr};
use serde_json::Value;

/// A lowercase word: letters, digits and `-`.
pub fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Every string inside a JSON value, for its templates.
pub fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

/// Compiles every `${...}` in a text.
pub fn check_templates(text: &str) -> Result<(), String> {
    for e in expr::templates(text)? {
        expr::compile(&e)?;
    }
    Ok(())
}

fn check_transition(t: &Transition, states: &[String]) -> Result<(), String> {
    let named = [
        ("from", Some(&t.from)),
        ("to", Some(&t.to)),
        ("on_timeout", t.on_timeout.as_ref()),
        ("on_fail", t.on_fail.as_ref()),
    ];
    for (what, s) in named {
        if let Some(s) = s
            && !states.contains(s)
        {
            return Err(format!("{what} \"{s}\" is not a state"));
        }
    }
    if t.on.is_none() && t.actions.is_empty() && t.from == t.to {
        return Err("a transition to its own state needs on, do, or both".into());
    }
    if let Some(on) = &t.on {
        let ok = on.split('.').all(is_word);
        if !ok {
            return Err(format!("on \"{on}\" is an event type, like photo.added"));
        }
    }
    match (&t.timeout, &t.on_timeout) {
        (Some(d), Some(_)) => {
            expr::duration(d)?;
        }
        (Some(_), None) => return Err("timeout needs on_timeout".into()),
        (None, Some(_)) => return Err("on_timeout needs timeout".into()),
        (None, None) => {}
    }
    if let Some(w) = &t.when {
        expr::compile(w)?;
    }
    for a in &t.actions {
        actions::check(&a.action, a.with.as_ref())?;
        if let Some(id) = &a.id
            && !is_word(id)
        {
            return Err(format!("id \"{id}\" is lowercase letters, digits and -"));
        }
        if let Some(w) = &a.when {
            expr::compile(w)?;
        }
        if let Some(e) = &a.expect {
            expr::compile(e)?;
        }
        if let Some(with) = &a.with {
            let mut texts = vec![];
            with.values().for_each(|v| strings(v, &mut texts));
            for text in texts {
                check_templates(text)?;
            }
        }
    }
    Ok(())
}

/// Checks a trigger: a schedule that reads as cron, or `on` an event type,
/// one of them.
pub fn check_trigger(t: &Trigger) -> Result<(), String> {
    match (&t.schedule, &t.on) {
        (Some(s), None) => crate::engine::schedule::check(s),
        (None, Some(on)) if on.split('.').all(is_word) => Ok(()),
        (None, Some(on)) => Err(format!(
            "trigger on \"{on}\" is an event type, like photo.added"
        )),
        _ => Err("a trigger has a schedule or on, one of them".into()),
    }
}

/// Checks a parsed workflow; `file` names it in errors (empty for text
/// that has no file).
pub fn validate(file: &str, f: &File) -> Result<(), WorkflowError> {
    let whole = |message: String| WorkflowError {
        file: file.to_string(),
        at: None,
        message,
    };
    if !is_word(&f.name) {
        return Err(whole(format!(
            "name \"{}\" is lowercase letters, digits and -",
            f.name
        )));
    }
    if f.states.is_empty() {
        return Err(whole("states is empty".into()));
    }
    for (i, s) in f.states.iter().enumerate() {
        if !is_word(s) {
            return Err(whole(format!(
                "state \"{s}\" is lowercase letters, digits and -"
            )));
        }
        if f.states[..i].contains(s) {
            return Err(whole(format!("state \"{s}\" is listed twice")));
        }
        if s == FAILED {
            return Err(whole(
                "\"failed\" is the engine's own state; do not list it".into(),
            ));
        }
    }
    for t in &f.triggers {
        check_trigger(t).map_err(&whole)?;
    }
    if !f.states.contains(&f.start) {
        return Err(whole(format!("start \"{}\" is not a state", f.start)));
    }
    if let Some(e) = f.end.iter().find(|e| !f.states.contains(e)) {
        return Err(whole(format!("end \"{e}\" is not a state")));
    }
    let mut ids: Vec<&str> = vec![];
    for t in &f.transitions {
        check_transition(t, &f.states).map_err(|message| WorkflowError {
            file: file.to_string(),
            at: Some(format!("transition \"{} -> {}\"", t.from, t.to)),
            message,
        })?;
        for id in t.actions.iter().filter_map(|a| a.id.as_deref()) {
            if ids.contains(&id) {
                return Err(whole(format!("id \"{id}\" is used twice")));
            }
            ids.push(id);
        }
    }
    Ok(())
}

/// Checks that every workspace action a workflow names is among `defs`.
pub fn actions(file: &str, f: &File, defs: &[Definition]) -> Result<(), WorkflowError> {
    for t in &f.transitions {
        for a in &t.actions {
            if actions::BUILT_IN.contains(&a.action.as_str()) {
                continue;
            }
            actions::find(&a.action, defs).map_err(|message| WorkflowError {
                file: file.to_string(),
                at: Some(format!("transition \"{} -> {}\"", t.from, t.to)),
                message,
            })?;
        }
    }
    Ok(())
}
