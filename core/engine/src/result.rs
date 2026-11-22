use serde::{Deserialize, Serialize};

use zeidex_indexing::{Candidate, CandidateKind};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchResult {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub path: String,
    pub score: i64,
    pub action: LaunchResultAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// How the query matched the name: `exact`, `prefix`, `suffix`,
    /// `contains`, `fuzzy` or `approximate`. Absent for browse rows, content
    /// hits and the web row, which are not name matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_kind: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LaunchResultAction {
    Open {
        path: String,
    },
    OpenFolder {
        path: String,
    },
    Reveal {
        path: String,
    },
    WebSearch {
        query: String,
    },
    Translate {
        text: String,
        target_lang: String,
    },
    /// A user-declared block, identified by its candidate id. There is nothing
    /// to open: the shell looks the block up and performs its steps.
    PerformBlock {
        candidate_id: String,
    },
}

impl From<(&Candidate, i64)> for LaunchResult {
    fn from((candidate, score): (&Candidate, i64)) -> Self {
        let action = match candidate.kind {
            CandidateKind::App => LaunchResultAction::Open {
                path: candidate.path.to_string(),
            },
            CandidateKind::File => LaunchResultAction::Open {
                path: candidate.path.to_string(),
            },
            CandidateKind::Folder => LaunchResultAction::OpenFolder {
                path: candidate.path.to_string(),
            },
            CandidateKind::Action => LaunchResultAction::PerformBlock {
                candidate_id: candidate.id.to_string(),
            },
        };

        Self {
            id: candidate.id.to_string(),
            kind: candidate.kind.to_string(),
            title: candidate.title.to_string(),
            subtitle: candidate.subtitle.as_deref().map(str::to_string),
            path: candidate.path.to_string(),
            score,
            action,
            icon: candidate.icon.as_deref().map(str::to_string),
            // Set by the engine when it knows which tier surfaced the row; the
            // plain conversion has no query to classify against.
            match_kind: None,
        }
    }
}
