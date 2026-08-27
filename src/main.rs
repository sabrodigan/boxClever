mod db;
mod sound;

use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};

const LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

const ROW_TITLE: u16 = 0;
const ROW_HINT: u16 = 1;
const ROW_GAME: u16 = 2;
const ROW_SCORE: u16 = 3;
const ROW_BOARD: u16 = 5;
const ROW_PROMPT: u16 = 11;
const ROW_STATUS: u16 = 12;
const ROW_RESULT: u16 = 13;
const ROW_AGAIN: u16 = 14;
const COL_CELL0: u16 = 5;
const CELL_STRIDE: u16 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    X,
    O,
}

impl Mark {
    fn other(self) -> Self {
        match self {
            Mark::X => Mark::O,
            Mark::O => Mark::X,
        }
    }

    fn as_char(self) -> char {
        match self {
            Mark::X => 'X',
            Mark::O => 'O',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    Taken(Mark),
}

struct Board {
    cells: [Cell; 9],
}

impl Board {
    fn new() -> Self {
        Self {
            cells: [Cell::Empty; 9],
        }
    }

    fn place(&mut self, index: usize, mark: Mark) -> bool {
        if index >= 9 {
            return false;
        }
        if self.cells[index] != Cell::Empty {
            return false;
        }
        self.cells[index] = Cell::Taken(mark);
        true
    }

    fn winner(&self) -> Option<Mark> {
        for line in LINES {
            let [a, b, c] = line;
            if let (Cell::Taken(x), Cell::Taken(y), Cell::Taken(z)) =
                (self.cells[a], self.cells[b], self.cells[c])
            {
                if x == y && y == z {
                    return Some(x);
                }
            }
        }
        None
    }

    fn is_full(&self) -> bool {
        self.cells.iter().all(|cell| *cell != Cell::Empty)
    }

    fn render(&self) -> String {
        let squares: Vec<String> = self
            .cells
            .iter()
            .enumerate()
            .map(|(i, cell)| match cell {
                Cell::Empty => format!("{}", i + 1),
                Cell::Taken(mark) => mark.as_char().to_string(),
            })
            .collect();

        format!(
            "     {} | {} | {}\n    ---+---+---\n     {} | {} | {}\n    ---+---+---\n     {} | {} | {}",
            squares[0],
            squares[1],
            squares[2],
            squares[3],
            squares[4],
            squares[5],
            squares[6],
            squares[7],
            squares[8]
        )
    }
}

struct Score {
    player1: u32,
    player2: u32,
    draws: u32,
}

impl Score {
    fn new() -> Self {
        Self {
            player1: 0,
            player2: 0,
            draws: 0,
        }
    }

    fn games_played(&self) -> u32 {
        self.player1 + self.player2 + self.draws
    }
}

struct MoveRecord {
    player: u8,
    square: u8,
    think_time: Duration,
}

struct SessionStats {
    started_at: Instant,
    moves: Vec<MoveRecord>,
    occupied_attempts: u32,
    invalid_keys: u32,
    moves_this_game: u32,
    shortest_game: Option<u32>,
    longest_game: Option<u32>,
}

impl SessionStats {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            moves: Vec::new(),
            occupied_attempts: 0,
            invalid_keys: 0,
            moves_this_game: 0,
            shortest_game: None,
            longest_game: None,
        }
    }

    fn record_move(&mut self, player: u8, square_index: usize, think_time: Duration) {
        self.moves.push(MoveRecord {
            player,
            square: (square_index + 1) as u8,
            think_time,
        });
        self.moves_this_game += 1;
    }

    fn finish_game(&mut self) {
        let n = self.moves_this_game;
        self.shortest_game = Some(self.shortest_game.map_or(n, |s| s.min(n)));
        self.longest_game = Some(self.longest_game.map_or(n, |s| s.max(n)));
        self.moves_this_game = 0;
    }

    /// Turns this session into a plain, database-friendly summary. Returns
    /// `None` if no moves were made at all, since there is nothing worth
    /// remembering about an empty session.
    fn to_match_report(&self, score: &Score) -> Option<db::MatchReport> {
        let (fastest, slowest, average) = self.move_speed_summary()?;
        let (p1_avg, p2_avg) = self
            .player_averages()
            .unwrap_or((Duration::ZERO, Duration::ZERO));
        let (centre, corners, edges) = self.region_counts();

        Some(db::MatchReport {
            duration_secs: self.started_at.elapsed().as_secs_f64(),
            games_played: score.games_played(),
            player1_wins: score.player1,
            player2_wins: score.player2,
            draws: score.draws,
            total_moves: self.moves.len() as u32,
            average_think_ms: average.as_secs_f64() * 1000.0,
            fastest_think_ms: fastest.think_time.as_millis() as u64,
            slowest_think_ms: slowest.think_time.as_millis() as u64,
            player1_avg_think_ms: p1_avg.as_secs_f64() * 1000.0,
            player2_avg_think_ms: p2_avg.as_secs_f64() * 1000.0,
            shortest_game_moves: self.shortest_game,
            longest_game_moves: self.longest_game,
            occupied_attempts: self.occupied_attempts,
            invalid_keys: self.invalid_keys,
            centre_picks: centre,
            corner_picks: corners,
            edge_picks: edges,
        })
    }

    fn report_lines(&self, score: &Score, extra: &[String]) -> Vec<String> {
        let mut lines = vec![
            String::from("  MATCH REPORT"),
            String::new(),
            format!(
                "  Time on the board: {}.",
                format_play_time(self.started_at.elapsed())
            ),
            format!(
                "  Games played: {}. Player 1 won {}. Player 2 won {}. Draws: {}.",
                score.games_played(),
                score.player1,
                score.player2,
                score.draws
            ),
        ];

        if let Some((fastest, slowest, average)) = self.move_speed_summary() {
            lines.push(String::new());
            lines.push(format!(
                "  Quickest number chosen: {}, square {}, by player {}.",
                format_think_time(fastest.think_time),
                fastest.square,
                fastest.player
            ));
            lines.push(format!(
                "  Slowest number chosen: {}, square {}, by player {}.",
                format_think_time(slowest.think_time),
                slowest.square,
                slowest.player
            ));
            lines.push(format!(
                "  Average number chosen speed: {}.",
                format_think_time(average)
            ));
        }

        if let Some((p1, p2)) = self.player_averages() {
            lines.push(format!(
                "  Player 1 average: {}. Player 2 average: {}.",
                format_think_time(p1),
                format_think_time(p2)
            ));
        }

        if !self.moves.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "  {} in all.",
                counted(self.moves.len() as u32, "move", "moves")
            ));
            lines.push(self.square_summary());
            lines.push(self.region_summary());
        }

        if let (Some(short), Some(long)) = (self.shortest_game, self.longest_game) {
            if short == long {
                lines.push(format!(
                    "  Every game lasted {}.",
                    counted(short, "move", "moves")
                ));
            } else {
                lines.push(format!(
                    "  Shortest game: {}. Longest game: {}.",
                    counted(short, "move", "moves"),
                    counted(long, "move", "moves")
                ));
            }
        }

        lines.push(format!(
            "  Occupied squares tapped: {}. Keys that were not 1-9: {}.",
            self.occupied_attempts, self.invalid_keys
        ));
        lines.push(String::new());
        lines.push(self.headline(score));

        if !extra.is_empty() {
            lines.push(String::new());
            lines.extend(extra.iter().cloned());
        }

        lines.push(String::from("  Thanks for playing."));
        lines.push(String::from("  Press any key to leave."));
        lines
    }

    fn move_speed_summary(&self) -> Option<(&MoveRecord, &MoveRecord, Duration)> {
        let fastest = self.moves.iter().min_by_key(|m| m.think_time)?;
        let slowest = self.moves.iter().max_by_key(|m| m.think_time)?;
        let total: Duration = self.moves.iter().map(|m| m.think_time).sum();
        let average = total / self.moves.len() as u32;
        Some((fastest, slowest, average))
    }

    fn player_averages(&self) -> Option<(Duration, Duration)> {
        let avg = |player: u8| -> Option<Duration> {
            let times: Vec<Duration> = self
                .moves
                .iter()
                .filter(|m| m.player == player)
                .map(|m| m.think_time)
                .collect();
            if times.is_empty() {
                None
            } else {
                Some(times.iter().copied().sum::<Duration>() / times.len() as u32)
            }
        };
        Some((avg(1)?, avg(2)?))
    }

    fn square_summary(&self) -> String {
        let mut counts = [0u32; 9];
        for mv in &self.moves {
            counts[(mv.square - 1) as usize] += 1;
        }
        let max = *counts.iter().max().unwrap_or(&0);
        let min = *counts.iter().min().unwrap_or(&0);
        let favourites = numbered_squares_with_count(&counts, max);
        let least = numbered_squares_with_count(&counts, min);
        format!("  Favourite square: {favourites}. Least used: {least}.")
    }

    fn region_summary(&self) -> String {
        let (centre, corners, edges) = self.region_counts();
        format!(
            "  The centre was picked {}. Corners {}. Edges {}.",
            counted(centre, "time", "times"),
            counted(corners, "time", "times"),
            counted(edges, "time", "times")
        )
    }

    fn region_counts(&self) -> (u32, u32, u32) {
        let mut centre = 0;
        let mut corners = 0;
        let mut edges = 0;
        for mv in &self.moves {
            match mv.square {
                5 => centre += 1,
                1 | 3 | 7 | 9 => corners += 1,
                _ => edges += 1,
            }
        }
        (centre, corners, edges)
    }

    fn headline(&self, score: &Score) -> String {
        match score.player1.cmp(&score.player2) {
            std::cmp::Ordering::Greater => {
                format!(
                    "  Player 1 takes the match, {} to {}.",
                    score.player1, score.player2
                )
            }
            std::cmp::Ordering::Less => {
                format!(
                    "  Player 2 takes the match, {} to {}.",
                    score.player2, score.player1
                )
            }
            std::cmp::Ordering::Equal if score.games_played() == 0 => {
                String::from("  No games were finished.")
            }
            std::cmp::Ordering::Equal if score.player1 == 0 => {
                String::from("  A session of draws. Honour in stalemate.")
            }
            std::cmp::Ordering::Equal => String::from("  The match is even. Well fought."),
        }
    }
}

struct Ui {
    interactive: bool,
}

impl Ui {
    fn new() -> Self {
        Self {
            interactive: io::stdout().is_terminal(),
        }
    }

    fn enter_game(&self) {
        if self.interactive {
            clear_screen();
            let _ = execute!(io::stdout(), Hide);
        }
        self.write_at(ROW_TITLE, "  TIC-TAC-TOE");
        self.write_at(ROW_HINT, "  Tap 1-9 to play. Symbols swap each game.");
        self.draw_grid();
    }

    fn draw_grid(&self) {
        self.write_at(ROW_BOARD, "     1 | 2 | 3");
        self.write_at(ROW_BOARD + 1, "    ---+---+---");
        self.write_at(ROW_BOARD + 2, "     4 | 5 | 6");
        self.write_at(ROW_BOARD + 3, "    ---+---+---");
        self.write_at(ROW_BOARD + 4, "     7 | 8 | 9");
    }

    fn reset_board(&self) {
        for index in 0..9 {
            self.paint_cell(index, char::from(b'1' + index as u8));
        }
    }

    fn paint_cell(&self, index: usize, ch: char) {
        if !self.interactive {
            return;
        }
        let (col, row) = cell_pos(index);
        let mut out = io::stdout();
        let _ = execute!(out, MoveTo(col, row));
        let _ = write!(out, "{ch}");
        let _ = out.flush();
    }

    fn set_game(&self, game_number: u32, player1_mark: Mark) {
        self.write_at(
            ROW_GAME,
            &format!(
                "  Game {game_number} — Player 1 is {}   Player 2 is {}",
                player1_mark.as_char(),
                player1_mark.other().as_char()
            ),
        );
    }

    fn set_score(&self, score: &Score) {
        self.write_at(
            ROW_SCORE,
            &format!(
                "  Score — Player 1: {}   Player 2: {}   Draws: {}",
                score.player1, score.player2, score.draws
            ),
        );
    }

    fn set_prompt(&self, text: &str) {
        self.write_at(ROW_PROMPT, text);
        if self.interactive {
            let col = text.chars().count() as u16;
            let _ = execute!(io::stdout(), MoveTo(col, ROW_PROMPT), Show);
        }
    }

    fn set_status(&self, text: &str) {
        self.write_at(ROW_STATUS, text);
        if self.interactive {
            let _ = execute!(io::stdout(), Hide);
        }
    }

    fn clear_messages(&self) {
        self.write_at(ROW_PROMPT, "");
        self.write_at(ROW_STATUS, "");
        self.write_at(ROW_RESULT, "");
        self.write_at(ROW_AGAIN, "");
        if self.interactive {
            let _ = execute!(io::stdout(), Hide);
        }
    }

    fn type_result(&self, text: &str) {
        self.write_at(ROW_RESULT, "");
        if self.interactive {
            let _ = execute!(io::stdout(), Hide, MoveTo(0, ROW_RESULT));
        }
        type_text(text);
        if self.interactive {
            thread::sleep(Duration::from_millis(400));
        } else {
            println!();
        }
    }

    fn type_report(&self, lines: &[String]) {
        if self.interactive {
            clear_screen();
            let _ = execute!(io::stdout(), Hide);
            for (i, line) in lines.iter().enumerate() {
                let _ = execute!(io::stdout(), MoveTo(0, i as u16));
                type_text_with_pace(line, 0.55);
                thread::sleep(Duration::from_millis(if line.is_empty() { 80 } else { 180 }));
            }
            let _ = execute!(io::stdout(), Show);
        } else {
            println!();
            for line in lines {
                println!("{line}");
            }
        }
    }

    fn write_at(&self, row: u16, text: &str) {
        if self.interactive {
            let mut out = io::stdout();
            let _ = execute!(out, MoveTo(0, row));
            let _ = write!(out, "{text}");
            let _ = execute!(out, Clear(ClearType::UntilNewLine));
            let _ = out.flush();
        } else if !text.is_empty() {
            println!("{text}");
        }
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn main() {
    // Opened before anything is drawn: bringing up the audio device can spill
    // ALSA chatter onto stderr, and the welcome screen clears it away.
    sound::init();

    show_welcome();

    let ui = Ui::new();
    ui.enter_game();

    let mut score = Score::new();
    let mut stats = SessionStats::new();
    let mut player1_mark = Mark::X;
    let mut game_number = 1;

    loop {
        play_round(&ui, game_number, player1_mark, &mut score, &mut stats);

        if !ask_play_again(&ui) {
            let extra = stats
                .to_match_report(&score)
                .and_then(|report| db::record_and_compare(&report))
                .map(|save| {
                    let mut lines = save.comparisons;
                    lines.push(format!("  Saved to the archive — key {}.", save.reference));
                    lines
                })
                .unwrap_or_default();

            ui.type_report(&stats.report_lines(&score, &extra));
            if ui.interactive {
                let _ = read_key();
            }
            break;
        }

        player1_mark = player1_mark.other();
        game_number += 1;
    }
}

fn play_round(
    ui: &Ui,
    game_number: u32,
    player1_mark: Mark,
    score: &mut Score,
    stats: &mut SessionStats,
) {
    let mut board = Board::new();
    let mut current = Mark::X;

    ui.clear_messages();
    ui.set_game(game_number, player1_mark);
    ui.set_score(score);
    ui.reset_board();

    loop {
        let player = player_name_for_mark(player1_mark, current);
        let player_num = player_number_for_mark(player1_mark, current);
        let turn_started = Instant::now();
        let choice = loop {
            let choice = ask_square(ui, player, current, stats);
            if board.place(choice, current) {
                break choice;
            }
            stats.occupied_attempts += 1;
            ui.set_status("  That square is already taken. Try again.");
        };
        stats.record_move(player_num, choice, turn_started.elapsed());

        ui.paint_cell(choice, current.as_char());
        if !ui.interactive {
            println!("{}", board.render());
            println!();
        }
        ui.set_status("");

        if let Some(winner) = board.winner() {
            ui.write_at(ROW_PROMPT, "");
            if ui.interactive {
                thread::sleep(Duration::from_millis(280));
            }
            let number = player_number_for_mark(player1_mark, winner);
            ui.type_result(&format!("  Well done to player {number}."));
            if winner == player1_mark {
                score.player1 += 1;
            } else {
                score.player2 += 1;
            }
            ui.set_score(score);
            stats.finish_game();
            return;
        }

        if board.is_full() {
            ui.write_at(ROW_PROMPT, "");
            ui.type_result("  It's a draw.");
            score.draws += 1;
            ui.set_score(score);
            stats.finish_game();
            return;
        }

        current = current.other();
    }
}

fn player_name_for_mark(player1_mark: Mark, mark: Mark) -> &'static str {
    if mark == player1_mark {
        "Player 1"
    } else {
        "Player 2"
    }
}

fn player_number_for_mark(player1_mark: Mark, mark: Mark) -> u8 {
    if mark == player1_mark {
        1
    } else {
        2
    }
}

fn ask_square(ui: &Ui, player: &str, mark: Mark, stats: &mut SessionStats) -> usize {
    ui.set_prompt(&format!(
        "  {player} ({}), choose a square (1-9): ",
        mark.as_char()
    ));

    loop {
        match read_key() {
            None => quit_game(),
            Some(KeyCode::Char(c)) if c.is_ascii_digit() && c != '0' => {
                if ui.interactive {
                    let _ = execute!(io::stdout(), Hide);
                }
                return (c as u8 - b'1') as usize;
            }
            Some(_) => {
                stats.invalid_keys += 1;
                ui.set_status("  Please press a number from 1 to 9.");
                ui.set_prompt(&format!(
                    "  {player} ({}), choose a square (1-9): ",
                    mark.as_char()
                ));
            }
        }
    }
}

fn ask_play_again(ui: &Ui) -> bool {
    loop {
        ui.set_prompt("");
        ui.set_status("");
        ui.write_at(ROW_AGAIN, "  Play again? [Y/n]: ");
        if ui.interactive {
            let prompt = "  Play again? [Y/n]: ";
            let _ = execute!(
                io::stdout(),
                MoveTo(prompt.chars().count() as u16, ROW_AGAIN),
                Show
            );
        }

        match read_key() {
            None => return false,
            Some(KeyCode::Enter) | Some(KeyCode::Char('\n' | '\r' | 'y' | 'Y')) => {
                ui.write_at(ROW_AGAIN, "  Play again? [Y/n]: Y");
                if ui.interactive {
                    let _ = execute!(io::stdout(), Hide);
                }
                return true;
            }
            Some(KeyCode::Char('n' | 'N')) => {
                ui.write_at(ROW_AGAIN, "  Play again? [Y/n]: n");
                if ui.interactive {
                    let _ = execute!(io::stdout(), Hide);
                }
                return false;
            }
            Some(_) => {
                ui.set_status("  Press Enter or Y for yes, N for no.");
            }
        }
    }
}

fn read_key() -> Option<KeyCode> {
    if io::stdin().is_terminal() {
        read_key_raw().ok()
    } else {
        read_key_from_line()
    }
}

fn read_key_raw() -> io::Result<KeyCode> {
    let _raw = RawMode::enter()?;
    loop {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c' | 'C'))
                {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"));
                }
                return Ok(key.code);
            }
            Event::Key(_) => {}
            _ => {}
        }
    }
}

fn read_key_from_line() -> Option<KeyCode> {
    let mut buf = String::new();
    match io::stdin().read_line(&mut buf) {
        Ok(0) => None,
        Ok(_) => {
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                Some(KeyCode::Enter)
            } else {
                trimmed.chars().next().map(KeyCode::Char)
            }
        }
        Err(_) => None,
    }
}

fn show_welcome() {
    clear_screen();
    println!();
    type_text("  Welcome to the game of TIC-TAC-TOE.");
    println!();
    if io::stdout().is_terminal() {
        thread::sleep(Duration::from_millis(280));
    }
    type_text("  It is a game of the ages.");
    println!();
    if io::stdout().is_terminal() {
        thread::sleep(Duration::from_millis(280));
    }
    type_text("  Can you win?");
    println!();
    println!();
    if io::stdout().is_terminal() {
        thread::sleep(Duration::from_millis(700));
    }
}

fn type_text(text: &str) {
    type_text_with_pace(text, 1.0);
}

fn type_text_with_pace(text: &str, pace: f64) {
    let mut out = io::stdout();
    let animate = io::stdout().is_terminal();

    for ch in text.chars() {
        let _ = write!(out, "{ch}");
        let _ = out.flush();
        if animate {
            // The operator in the back room taps the key as the letter lands.
            sound::key_press(ch);
            let delay = typewriter_delay(ch).as_secs_f64() * pace;
            thread::sleep(Duration::from_secs_f64(delay));
        }
    }
}

fn typewriter_delay(ch: char) -> Duration {
    let millis = match ch {
        '.' | '?' | '!' => 220,
        ',' | ';' => 120,
        '-' => 90,
        ' ' => 45,
        _ => 38,
    };
    Duration::from_millis(millis)
}

fn format_think_time(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        counted(millis as u32, "millisecond", "milliseconds")
    } else {
        format!("{:.2} seconds", duration.as_secs_f64())
    }
}

fn format_play_time(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    if total_secs < 1 {
        counted(duration.as_millis() as u32, "millisecond", "milliseconds")
    } else if total_secs < 60 {
        format!("{:.1} seconds", duration.as_secs_f64())
    } else {
        let minutes = total_secs / 60;
        let seconds = total_secs % 60;
        format!(
            "{} and {}",
            counted(minutes as u32, "minute", "minutes"),
            counted(seconds as u32, "second", "seconds")
        )
    }
}

fn counted(n: u32, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {plural}")
    }
}

fn numbered_squares_with_count(counts: &[u32; 9], target: u32) -> String {
    let squares: Vec<String> = counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == target)
        .map(|(i, _)| (i + 1).to_string())
        .collect();
    format!(
        "{} ({})",
        join_english(&squares),
        counted(target, "time", "times")
    )
}

fn join_english(items: &[String]) -> String {
    match items.len() {
        0 => String::from("none"),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        n => format!("{} and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

fn cell_pos(index: usize) -> (u16, u16) {
    let col = COL_CELL0 + (index as u16 % 3) * CELL_STRIDE;
    let row = ROW_BOARD + (index as u16 / 3) * 2;
    (col, row)
}

fn clear_screen() {
    if !io::stdout().is_terminal() {
        return;
    }
    let mut out = io::stdout();
    let _ = execute!(out, Clear(ClearType::All), MoveTo(0, 0));
}

fn restore_terminal() {
    if io::stdout().is_terminal() {
        let _ = execute!(io::stdout(), Show);
        let _ = disable_raw_mode();
    }
}

fn quit_game() -> ! {
    restore_terminal();
    std::process::exit(0);
}

struct RawMode;

impl RawMode {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}
