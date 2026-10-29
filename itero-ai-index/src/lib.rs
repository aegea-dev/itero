//! The models a machine has, as itero's `#find-models` writes them and
//! itero-ai reads them, and how a step's model name picks one.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One model: a GGUF file (or the first part of one in parts), named by
/// where it comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// `<org>/<repo>:<QUANT>`.
    pub name: String,
    /// `<org>/<repo>`.
    pub repo: String,
    pub quant: String,
    /// Every copy of it on this machine; the first is used.
    pub paths: Vec<PathBuf>,
    pub maker: String,
    pub params: Option<String>,
    pub architecture: String,
    pub context: Option<u64>,
    pub bytes: u64,
    /// `unknown` when the file names none.
    pub licence: String,
    /// It has a chat template.
    pub chat: bool,
    /// This build of llama.cpp knows its architecture.
    pub runs: bool,
    /// itero-ai splits its thinking from its answer.
    pub thinking: bool,
    /// The file's modification time (seconds): it is inspected again when
    /// this or its size changes.
    pub modified: u64,
    /// Written by `itero ai models add`; the scan keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<Downloaded>,
    /// When `itero ai models remove` took it out of use; the scan keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<String>,
}

/// Where a model itero downloaded came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Downloaded {
    /// The repo's page: `https://huggingface.co/<org>/<repo>`.
    pub from: String,
    pub file: String,
    /// The repo's commit it was downloaded at.
    pub revision: String,
    /// What Hugging Face published, and the file was checked against.
    pub sha256: String,
    pub at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub models: Vec<Entry>,
}

impl Index {
    /// The index at `path`; none there yet is an empty one.
    pub fn read(path: &Path) -> anyhow::Result<Index> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                serde_json::from_str(&t).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Index::default()),
            Err(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
        }
    }

    /// Writes it whole: a new file, then a rename, so it is never read
    /// half written.
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
        }
        let new = path.with_extension("json.new");
        std::fs::write(&new, serde_json::to_string_pretty(self)? + "\n")
            .map_err(|e| anyhow::anyhow!("{}: {e}", new.display()))?;
        std::fs::rename(&new, path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub enum ResolveError {
    NotFound(String),
    /// The repo has several files and none is `Q4_K_M`.
    Choose {
        repo: String,
        quants: Vec<String>,
    },
    Removed(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound(n) => write!(f, "no model {n} on this machine"),
            ResolveError::Choose { repo, quants } => write!(
                f,
                "{repo} has {}: name one, like {repo}:{}",
                quants.join(", "),
                quants[0]
            ),
            ResolveError::Removed(n) => {
                write!(f, "{n} was removed (itero ai models add {n} takes it back)")
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// The model `name` picks, for a step to use: a removed one is an error.
pub fn resolve<'a>(ix: &'a Index, name: &str) -> Result<&'a Entry, ResolveError> {
    let e = pick(ix, name)?;
    match e.removed {
        Some(_) => Err(ResolveError::Removed(e.name.clone())),
        None => Ok(e),
    }
}

/// The entry `name` picks, removed or not: `<org>/<repo>:<QUANT>` that
/// file (in any case); `<org>/<repo>` its only file, else `Q4_K_M` as
/// llama.cpp chooses, else an error naming them.
pub fn pick<'a>(ix: &'a Index, name: &str) -> Result<&'a Entry, ResolveError> {
    let (repo, quant) = match name.rsplit_once(':') {
        Some((r, q)) => (r, Some(q)),
        None => (name, None),
    };
    let same: Vec<&Entry> = ix.models.iter().filter(|e| e.repo == repo).collect();
    match quant {
        Some(q) => same
            .into_iter()
            .find(|e| e.quant.eq_ignore_ascii_case(q))
            .ok_or_else(|| ResolveError::NotFound(name.into())),
        None => match same.as_slice() {
            [] => Err(ResolveError::NotFound(name.into())),
            [only] => Ok(only),
            many => many
                .iter()
                .find(|e| e.quant.eq_ignore_ascii_case("Q4_K_M"))
                .copied()
                .ok_or_else(|| ResolveError::Choose {
                    repo: repo.into(),
                    quants: many.iter().map(|e| e.quant.clone()).collect(),
                }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(repo: &str, quant: &str) -> Entry {
        Entry {
            name: format!("{repo}:{quant}"),
            repo: repo.into(),
            quant: quant.into(),
            paths: vec![format!("/modeller/{repo}/{quant}.gguf").into()],
            maker: repo.split('/').next().unwrap().into(),
            params: Some("4B".into()),
            architecture: "qwen35".into(),
            context: Some(262144),
            bytes: 1,
            licence: "apache-2.0".into(),
            chat: true,
            runs: true,
            thinking: true,
            modified: 0,
            downloaded: None,
            removed: None,
        }
    }

    #[test]
    fn a_name_with_its_quantization_is_that_file_without_case() {
        let ix = Index {
            models: vec![e("unsloth/Qwen3.5-4B-GGUF", "Q4_K_M")],
        };
        assert_eq!(
            resolve(&ix, "unsloth/Qwen3.5-4B-GGUF:q4_k_m")
                .unwrap()
                .quant,
            "Q4_K_M"
        );
    }

    #[test]
    fn without_one_the_only_file_else_q4_k_m_else_the_choices() {
        let one = Index {
            models: vec![e("örnek/tek-GGUF", "Q8_0")],
        };
        assert_eq!(resolve(&one, "örnek/tek-GGUF").unwrap().quant, "Q8_0");
        let two = Index {
            models: vec![e("u/çift", "Q3_K_M"), e("u/çift", "Q4_K_M")],
        };
        assert_eq!(resolve(&two, "u/çift").unwrap().quant, "Q4_K_M");
        let none = Index {
            models: vec![e("u/şey", "Q3_K_M"), e("u/şey", "Q8_0")],
        };
        let err = resolve(&none, "u/şey").unwrap_err().to_string();
        assert!(err.contains("Q3_K_M") && err.contains("Q8_0"), "{err}");
        assert!(matches!(
            resolve(&none, "u/yok"),
            Err(ResolveError::NotFound(_))
        ));
        assert!(matches!(
            resolve(&none, "u/şey:Q5_K_M"),
            Err(ResolveError::NotFound(_))
        ));
    }

    #[test]
    fn a_removed_model_is_not_used_but_still_picked() {
        let mut m = e("u/eski", "Q4_K_M");
        m.removed = Some("2026-10-29T10:00:00Z".into());
        let ix = Index { models: vec![m] };
        let err = resolve(&ix, "u/eski").unwrap_err();
        assert!(matches!(err, ResolveError::Removed(_)));
        assert!(err.to_string().contains("itero ai models add"), "{err}");
        assert_eq!(pick(&ix, "u/eski").unwrap().quant, "Q4_K_M");
    }

    #[test]
    fn the_index_is_written_whole_and_a_missing_one_is_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("önbellek").join("models.json");
        assert!(Index::read(&p).unwrap().models.is_empty());
        let mut m = e("u/ağaç", "Q4_K_M");
        m.downloaded = Some(Downloaded {
            from: "https://huggingface.co/u/ağaç".into(),
            file: "ağaç-Q4_K_M.gguf".into(),
            revision: "abc".into(),
            sha256: "00".into(),
            at: "2026-10-29T14:02:11Z".into(),
        });
        let ix = Index { models: vec![m] };
        ix.write(&p).unwrap();
        assert_eq!(Index::read(&p).unwrap(), ix);
        assert!(!p.with_extension("json.new").exists());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("removed"), "an unset mark is not written");
    }
}
