//! Writes each session's match report to a local MongoDB server and compares
//! it against every report written before it.
//!
//! This is a hint of history, not a critical system: if there is no server
//! listening, or anything about the connection goes wrong, every function
//! here returns `None` and the game carries on exactly as it would without
//! a database at all. Nothing is ever surfaced to the player except a
//! successful save.

use std::time::Duration;

use mongodb::bson::doc;
use mongodb::options::ClientOptions;
use mongodb::sync::{Client, Collection};
use serde::{Deserialize, Serialize};

const DEFAULT_URI: &str = "mongodb://localhost:27017";
const DEFAULT_DB: &str = "boxclever";
const COLLECTION_NAME: &str = "match_reports";

/// Give up quickly if nothing answers; this runs inline in an interactive
/// terminal game, so it must never make the player sit through a long
/// default driver timeout just because the server isn't running.
const SERVER_SELECTION_TIMEOUT: Duration = Duration::from_millis(500);

/// A plain summary of one play session. Knows nothing about `Board`, `Mark`,
/// or any other game internals — `main.rs` builds one of these from
/// `SessionStats` and `Score` once a session ends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchReport {
    pub duration_secs: f64,
    pub games_played: u32,
    pub player1_wins: u32,
    pub player2_wins: u32,
    pub draws: u32,
    pub total_moves: u32,
    pub average_think_ms: f64,
    pub fastest_think_ms: u64,
    pub slowest_think_ms: u64,
    pub player1_avg_think_ms: f64,
    pub player2_avg_think_ms: f64,
    pub shortest_game_moves: Option<u32>,
    pub longest_game_moves: Option<u32>,
    pub occupied_attempts: u32,
    pub invalid_keys: u32,
    pub centre_picks: u32,
    pub corner_picks: u32,
    pub edge_picks: u32,
}

/// What comes back from a successful save: a short reference the player can
/// see typed onto the screen as proof the write happened, plus whatever we
/// could work out by comparing this session to every one that came before it.
pub struct SaveResult {
    pub reference: String,
    pub comparisons: Vec<String>,
}

/// Save `report` to the local database and compare it against history.
///
/// Returns `None` if a MongoDB server can't be reached or the write fails
/// for any reason — the caller should treat that exactly like there being no
/// database at all.
pub fn record_and_compare(report: &MatchReport) -> Option<SaveResult> {
    let collection = connect()?;

    // Read the history *before* inserting this session, so the comparisons
    // below are against everything that came before, not including itself.
    let history: Vec<MatchReport> = collection
        .find(doc! {})
        .run()
        .ok()?
        .filter_map(|doc| doc.ok())
        .collect();

    let comparisons = build_comparisons(report, &history);

    let inserted = collection.insert_one(report).run().ok()?;
    let reference = inserted
        .inserted_id
        .as_object_id()
        .map(|id| id.to_hex())
        .unwrap_or_else(|| inserted.inserted_id.to_string());

    Some(SaveResult {
        reference,
        comparisons,
    })
}

fn connect() -> Option<Collection<MatchReport>> {
    let uri = std::env::var("BOXCLEVER_MONGO_URI").unwrap_or_else(|_| DEFAULT_URI.to_string());
    let db_name = std::env::var("BOXCLEVER_MONGO_DB").unwrap_or_else(|_| DEFAULT_DB.to_string());

    let mut options = ClientOptions::parse(&uri).run().ok()?;
    options.server_selection_timeout = Some(SERVER_SELECTION_TIMEOUT);
    options.connect_timeout = Some(SERVER_SELECTION_TIMEOUT);

    let client = Client::with_options(options).ok()?;
    Some(client.database(&db_name).collection(COLLECTION_NAME))
}

fn build_comparisons(current: &MatchReport, history: &[MatchReport]) -> Vec<String> {
    let mut lines = Vec::new();

    if history.is_empty() {
        return lines;
    }

    let session_number = history.len() + 1;
    lines.push(format!("  This was session number {session_number} on this machine."));

    speed_comparison(current, history, &mut lines);
    score_comparison(current, history, &mut lines);
    standings_comparison(current, history, &mut lines);
    game_length_comparison(current, history, &mut lines);
    playtime_comparison(current, history, &mut lines);

    lines
}

fn speed_comparison(current: &MatchReport, history: &[MatchReport], lines: &mut Vec<String>) {
    let past_average: f64 =
        history.iter().map(|r| r.average_think_ms).sum::<f64>() / history.len() as f64;
    let delta = past_average - current.average_think_ms;

    // A few percent either way reads as noise rather than a real change.
    if delta.abs() > past_average * 0.05 {
        if delta > 0.0 {
            lines.push(format!(
                "  You were faster on the keys tonight — {:.0}ms a move, against your usual {:.0}ms.",
                current.average_think_ms, past_average
            ));
        } else {
            lines.push(format!(
                "  You were slower on the keys tonight — {:.0}ms a move, against your usual {:.0}ms.",
                current.average_think_ms, past_average
            ));
        }
    } else {
        lines.push(String::from(
            "  Your pace on the keys was right about average tonight.",
        ));
    }

    let past_fastest = history.iter().map(|r| r.fastest_think_ms).min();
    if past_fastest.is_some_and(|fastest| current.fastest_think_ms < fastest) {
        lines.push(String::from(
            "  That included the quickest single move you've ever made.",
        ));
    }
}

fn score_comparison(current: &MatchReport, history: &[MatchReport], lines: &mut Vec<String>) {
    let matched_before = history.iter().any(|r| {
        r.player1_wins == current.player1_wins
            && r.player2_wins == current.player2_wins
            && r.draws == current.draws
    });
    if matched_before {
        lines.push(format!(
            "  You've landed on {}-{}-{} (wins-wins-draws) before.",
            current.player1_wins, current.player2_wins, current.draws
        ));
    }
}

fn standings_comparison(current: &MatchReport, history: &[MatchReport], lines: &mut Vec<String>) {
    let total_p1: u32 = current.player1_wins + history.iter().map(|r| r.player1_wins).sum::<u32>();
    let total_p2: u32 = current.player2_wins + history.iter().map(|r| r.player2_wins).sum::<u32>();

    match total_p1.cmp(&total_p2) {
        std::cmp::Ordering::Greater => lines.push(format!(
            "  All-time, Player 1 leads {total_p1} to {total_p2}."
        )),
        std::cmp::Ordering::Less => lines.push(format!(
            "  All-time, Player 2 leads {total_p2} to {total_p1}."
        )),
        std::cmp::Ordering::Equal => lines.push(format!(
            "  All-time, the two players are dead even at {total_p1} wins apiece."
        )),
    }
}

fn game_length_comparison(current: &MatchReport, history: &[MatchReport], lines: &mut Vec<String>) {
    if let Some(shortest) = current.shortest_game_moves {
        let past_shortest = history.iter().filter_map(|r| r.shortest_game_moves).min();
        if past_shortest.is_none_or(|past| shortest < past) {
            lines.push(format!(
                "  New record: your shortest game ever, in {shortest} moves."
            ));
        }
    }
    if let Some(longest) = current.longest_game_moves {
        let past_longest = history.iter().filter_map(|r| r.longest_game_moves).max();
        if past_longest.is_none_or(|past| longest > past) {
            lines.push(format!(
                "  New record: your longest game ever, going {longest} moves."
            ));
        }
    }
}

fn playtime_comparison(current: &MatchReport, history: &[MatchReport], lines: &mut Vec<String>) {
    let total: f64 = current.duration_secs + history.iter().map(|r| r.duration_secs).sum::<f64>();
    let minutes = total / 60.0;
    lines.push(format!(
        "  You've now spent {minutes:.1} minutes on this board across every session."
    ));
}
