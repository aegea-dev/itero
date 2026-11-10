//! `itero context`: the itero deployments this CLI talks to.
use super::out::{show, table, text};
use crate::context::{Context, Contexts, LOCAL, bad_name, check, server_url};
use anyhow::{Result, bail};
use serde_json::{Value, json};

/// The token for a context: ITERO_TOKEN when set (CI), else asked for
/// without echo; empty is none (a server on 127.0.0.1 needs none).
fn token(name: &str) -> Result<Option<String>> {
    let t = match std::env::var("ITERO_TOKEN") {
        Ok(t) => t,
        Err(_) => rpassword::prompt_password(format!("{name}'s token (empty for none): "))?,
    };
    Ok((!t.is_empty()).then_some(t))
}

/// `itero context add`: checks the server with the token, then keeps it.
pub fn add(name: &str, url: &str, ws: Option<String>) -> Result<()> {
    // every problem with what was given, at once
    let problems: Vec<String> = [bad_name(name), server_url(url).err()]
        .into_iter()
        .flatten()
        .collect();
    if !problems.is_empty() {
        bail!("{}", problems.join("\n"));
    }
    let url = server_url(url).map_err(anyhow::Error::msg)?;
    let token = token(name)?;
    let itero = check(name, &url, token.as_deref())?;
    let path = Contexts::path()?;
    let mut all = Contexts::read(&path)?;
    let replaced = all.upsert(Context {
        name: name.to_string(),
        url: url.clone(),
        token,
        ws,
    });
    all.write(&path)?;
    let did = if replaced { "replaced" } else { "added" };
    println!("{did} {name}: {url}, itero {itero}");
    Ok(())
}

/// `itero context use`: the context commands use from now on.
pub fn use_(name: &str) -> Result<()> {
    let path = Contexts::path()?;
    let mut all = Contexts::read(&path)?;
    if name == LOCAL {
        all.current = None;
    } else if all.get(name).is_some() {
        all.current = Some(name.to_string());
    } else {
        bail!("no context {name}: itero context list lists them");
    }
    all.write(&path)?;
    println!("using {name}");
    Ok(())
}

/// `itero context list`: each context, `local` first, the current one
/// marked; never a token.
pub fn list(json: bool) -> Result<()> {
    let all = Contexts::read(&Contexts::path()?)?;
    let current = all.current.as_deref().unwrap_or(LOCAL);
    let mut rows = vec![json!({ "name": LOCAL, "url": null, "ws": null,
                                "current": current == LOCAL })];
    rows.extend(all.contexts.iter().map(
        |c| json!({ "name": c.name, "url": c.url, "ws": c.ws, "current": current == c.name }),
    ));
    show(json, &Value::Array(rows), |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let mark = if c["current"] == true { "*" } else { " " };
                vec![
                    format!("{mark} {}", text(&c["name"])),
                    c["url"].as_str().unwrap_or("this machine").to_string(),
                    c["ws"].as_str().unwrap_or_default().to_string(),
                ]
            })
            .collect();
        table(&rows);
    })
}

/// `itero context remove`.
pub fn remove(name: &str) -> Result<()> {
    if name == LOCAL {
        bail!("{LOCAL} is this machine: it is built in");
    }
    let path = Contexts::path()?;
    let mut all = Contexts::read(&path)?;
    if !all.remove(name) {
        bail!("no context {name}: itero context list lists them");
    }
    all.write(&path)?;
    println!("removed {name}");
    Ok(())
}
