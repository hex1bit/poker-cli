//! JSONL 手牌历史导出。

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use crate::game::state::HandState;

#[derive(Debug, Clone)]
pub struct HandLogRecord {
    pub hand_no: u32,
    pub button: String,
    pub stage: String,
    pub board: Vec<String>,
    pub winners: Vec<String>,
    pub deltas: Vec<i64>,
    pub stacks: Vec<u64>,
    pub log: Vec<String>,
}

impl HandLogRecord {
    pub fn from_state(
        hand_no: u32,
        state: &HandState,
        winners: &[usize],
        deltas: &[i64],
        log: &[String],
    ) -> Self {
        Self {
            hand_no,
            button: state.players[state.button].name.clone(),
            stage: format!("{:?}", state.stage),
            board: state.community.iter().map(|c| c.to_string()).collect(),
            winners: winners
                .iter()
                .map(|&i| state.players[i].name.clone())
                .collect(),
            deltas: deltas.to_vec(),
            stacks: state.players.iter().map(|p| p.stack).collect(),
            log: log.to_vec(),
        }
    }

    pub fn append_jsonl(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(f, "{}", self.to_json_line())
    }

    fn to_json_line(&self) -> String {
        format!(
            "{{\"hand_no\":{},\"button\":{},\"stage\":{},\"board\":{},\"winners\":{},\"deltas\":{},\"stacks\":{},\"log\":{}}}",
            self.hand_no,
            json_string(&self.button),
            json_string(&self.stage),
            json_string_array(&self.board),
            json_string_array(&self.winners),
            json_i64_array(&self.deltas),
            json_u64_array(&self.stacks),
            json_string_array(&self.log),
        )
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_string_array(items: &[String]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| json_string(s))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_i64_array(items: &[i64]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_u64_array(items: &[u64]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_json_strings() {
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
