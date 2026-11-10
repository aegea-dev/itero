//! `itero token`: the tokens this server lets in, made on the server.
use super::out::{show, table, text};
use anyhow::Result;
use serde_json::{Value, json};

/// `itero token create`: the token on stdout, alone, for a script to
/// take; on stderr, that it is shown once.
pub fn create(name: &str, json: bool) -> Result<()> {
    let token = crate::token::create(name)?;
    let v = json!({ "name": name, "token": token });
    show(json, &v, |_| {
        println!("{token}");
        eprintln!(
            "{name}: shown this once, as this server keeps only its hash: itero context add, on the machine that will use it, takes it"
        );
    })
}

/// `itero token list`: names and when each was made, never a token.
pub fn list(json: bool) -> Result<()> {
    let rows: Vec<Value> = crate::token::list()?
        .into_iter()
        .map(|t| json!({ "name": t.name, "id": t.id, "created": t.created }))
        .collect();
    show(json, &Value::Array(rows), |v| {
        let rows: Vec<Vec<String>> = v
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| vec![text(&t["name"]), text(&t["created"])])
            .collect();
        if rows.is_empty() {
            println!("no tokens: itero token create NAME makes one");
        }
        table(&rows);
    })
}

/// `itero token revoke`.
pub fn revoke(name: &str) -> Result<()> {
    crate::token::revoke(name)?;
    println!("revoked {name}: it lets nobody in from now on");
    Ok(())
}
