//! Interactive TUI subscriber for Aeron market data.
//!
//! Displays live ticker, throughput sparkline, and stats.
//! Press keys to change parameters and watch throughput react in real-time.

use aeron_java_rust_bridge::{
    decode_price_exponent, decode_price_mantissa, decode_qty_exponent, decode_qty_mantissa,
    decode_symbol, decode_timestamp, is_buyer_maker, mantissa_to_f64, AERON_DIR, IPC_CHANNEL,
    MESSAGE_SIZE, STREAM_ID,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Sparkline},
    Frame, Terminal,
};
use rusteron_client::*;
use std::ffi::CString;
use std::io::stdout;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// =============================================================================
// SHARED STATE (atomics for lock-free communication between threads)
// =============================================================================

struct SharedParams {
    fragment_limit: AtomicU32,
    sleep_us: AtomicU32,
    idle_strategy: AtomicU32, // 0=BusySpin, 1=Yield, 2=Backoff
    paused: AtomicBool,
    running: AtomicBool,
    total_messages: AtomicU64,
    poll_count: AtomicU64,
}

impl SharedParams {
    fn new() -> Self {
        Self {
            fragment_limit: AtomicU32::new(10),
            sleep_us: AtomicU32::new(100),
            idle_strategy: AtomicU32::new(2), // Backoff default
            paused: AtomicBool::new(false),
            running: AtomicBool::new(true),
            total_messages: AtomicU64::new(0),
            poll_count: AtomicU64::new(0),
        }
    }
}

// =============================================================================
// TRADE MESSAGE (sent from worker → TUI thread)
// =============================================================================

struct TradeMsg {
    symbol: [u8; 8],
    price: f64,
    qty: f64,
    is_buy: bool,
    #[allow(dead_code)]
    timestamp: i64,
    seq: u64,
}

// =============================================================================
// TUI STATE
// =============================================================================

const FRAGMENT_PRESETS: [u32; 5] = [1, 10, 50, 256, 1024];
const SLEEP_PRESETS: [u32; 4] = [0, 10, 100, 1000];
const IDLE_NAMES: [&str; 3] = ["BusySpin", "Yield", "Backoff"];
const TICKER_SIZE: usize = 16;
const SPARKLINE_HISTORY: usize = 60; // 60 seconds of history

struct TuiState {
    ticker: Vec<TradeMsg>,
    throughput_history: Vec<u64>,
    last_total: u64,
    last_tick: Instant,
    current_rate: u64,
    peak_rate: u64,
    fragment_idx: usize,
    sleep_idx: usize,
    idle_idx: usize,
    start_time: Instant,
}

impl TuiState {
    fn new() -> Self {
        Self {
            ticker: Vec::with_capacity(TICKER_SIZE),
            throughput_history: vec![0; SPARKLINE_HISTORY],
            last_total: 0,
            last_tick: Instant::now(),
            current_rate: 0,
            peak_rate: 0,
            fragment_idx: 1, // starts at 10
            sleep_idx: 2,    // starts at 100μs
            idle_idx: 2,     // starts at Backoff
            start_time: Instant::now(),
        }
    }
}

// =============================================================================
// WORKER THREAD (Aeron polling)
// =============================================================================

fn worker_thread(params: Arc<SharedParams>, trade_tx: Sender<TradeMsg>) {
    // Connect to Aeron
    let ctx = match AeronContext::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to create Aeron context: {}", e);
            params.running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let aeron_dir = CString::new(AERON_DIR).unwrap();
    if let Err(e) = ctx.set_dir(&aeron_dir) {
        eprintln!("Failed to set Aeron dir: {}", e);
        params.running.store(false, Ordering::SeqCst);
        return;
    }

    let aeron = match Aeron::new(&ctx) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to create Aeron: {}", e);
            params.running.store(false, Ordering::SeqCst);
            return;
        }
    };

    if let Err(e) = aeron.start() {
        eprintln!("Failed to start Aeron: {}", e);
        params.running.store(false, Ordering::SeqCst);
        return;
    }

    let channel = CString::new(IPC_CHANNEL).unwrap();
    let subscription = match aeron
        .async_add_subscription(
            &channel,
            STREAM_ID,
            Handlers::no_available_image_handler(),
            Handlers::no_unavailable_image_handler(),
        )
        .and_then(|s| s.poll_blocking(Duration::from_secs(10)))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to create subscription: {}", e);
            params.running.store(false, Ordering::SeqCst);
            return;
        }
    };

    // Fragment handler
    struct TuiHandler {
        tx: Sender<TradeMsg>,
        count: std::cell::Cell<u64>,
    }

    /// Only send a sample to the TUI every N messages.
    /// Stats count everything; the ticker just shows snapshots.
    const TICKER_SAMPLE_INTERVAL: u64 = 10_000;

    impl AeronFragmentHandlerCallback for TuiHandler {
        fn handle_aeron_fragment_handler(&mut self, buffer: &[u8], _header: AeronHeader) {
            if buffer.len() < MESSAGE_SIZE {
                return;
            }

            let count = self.count.get() + 1;
            self.count.set(count);

            // Only decode + send every 10K messages to avoid flooding the TUI
            if count % TICKER_SAMPLE_INTERVAL != 0 {
                return;
            }

            let timestamp = decode_timestamp(buffer);
            let symbol_str = decode_symbol(buffer);
            let price =
                mantissa_to_f64(decode_price_mantissa(buffer), decode_price_exponent(buffer));
            let qty = mantissa_to_f64(decode_qty_mantissa(buffer), decode_qty_exponent(buffer));
            let is_buy = is_buyer_maker(buffer);

            let mut symbol = [b' '; 8];
            let bytes = symbol_str.as_bytes();
            symbol[..bytes.len().min(8)].copy_from_slice(&bytes[..bytes.len().min(8)]);

            // Non-blocking send — drop if TUI can't keep up (that's fine)
            let _ = self.tx.try_send(TradeMsg {
                symbol,
                price,
                qty,
                is_buy,
                timestamp,
                seq: count,
            });
        }
    }

    let (handler, inner) = match Handler::leak_with_fragment_assembler(TuiHandler {
        tx: trade_tx,
        count: std::cell::Cell::new(0),
    }) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to create handler: {}", e);
            params.running.store(false, Ordering::SeqCst);
            return;
        }
    };

    // Backoff state for idle strategy
    let mut spin_count: u32 = 0;
    let mut yield_count: u32 = 0;

    // Main poll loop
    while params.running.load(Ordering::Relaxed) {
        if params.paused.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }

        let frag_limit = params.fragment_limit.load(Ordering::Relaxed);
        let result = subscription.poll(Some(&handler), frag_limit as usize);

        let fragments = match result {
            Ok(n) => n,
            Err(_) => 0,
        };

        // Update total message count
        params
            .total_messages
            .store(inner.count.get(), Ordering::Relaxed);
        params.poll_count.fetch_add(1, Ordering::Relaxed);

        // Apply idle strategy when no messages
        if fragments == 0 {
            let strategy = params.idle_strategy.load(Ordering::Relaxed);
            match strategy {
                0 => {
                    // BusySpin
                    std::hint::spin_loop();
                }
                1 => {
                    // Yield
                    std::thread::yield_now();
                }
                2 => {
                    // Backoff: spin → yield → sleep
                    if spin_count < 100 {
                        spin_count += 1;
                        std::hint::spin_loop();
                    } else if yield_count < 10 {
                        yield_count += 1;
                        std::thread::yield_now();
                    } else {
                        let sleep_us = params.sleep_us.load(Ordering::Relaxed);
                        if sleep_us > 0 {
                            std::thread::sleep(Duration::from_micros(sleep_us as u64));
                        } else {
                            std::thread::yield_now();
                        }
                    }
                }
                _ => {}
            }
        } else {
            spin_count = 0;
            yield_count = 0;

            // Even after getting messages, apply configured sleep
            let sleep_us = params.sleep_us.load(Ordering::Relaxed);
            if sleep_us > 0 {
                std::thread::sleep(Duration::from_micros(sleep_us as u64));
            }
        }
    }
}

// =============================================================================
// TUI RENDERING
// =============================================================================

fn render(frame: &mut Frame, state: &TuiState, params: &SharedParams) {
    // Main layout: left (ticker + sparkline) | right (controls + stats)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(frame.area());

    // Left side: ticker on top, sparkline on bottom
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(8)])
        .split(main_chunks[0]);

    // Right side: controls on top, stats on bottom
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(12), Constraint::Min(8)])
        .split(main_chunks[1]);

    render_ticker(frame, left_chunks[0], state);
    render_sparkline(frame, left_chunks[1], state);
    render_controls(frame, right_chunks[0], state, params);
    render_stats(frame, right_chunks[1], state, params);
}

fn render_ticker(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" Live Ticker ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            format!(
                " {:>10} {:>8}  {:>4}  {:>12}  {:>10}",
                "Seq", "Symbol", "Side", "Price", "Qty"
            ),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    for trade in state.ticker.iter().rev() {
        let symbol = std::str::from_utf8(&trade.symbol)
            .unwrap_or("???")
            .trim();
        let (side_str, side_color, arrow) = if trade.is_buy {
            ("BUY ", Color::Green, "▲")
        } else {
            ("SELL", Color::Red, "▼")
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:>10} ", trade.seq),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(format!("{:>8}", symbol), Style::default().fg(Color::White)),
            Span::styled(
                format!("  {}{} ", side_str, arrow),
                Style::default()
                    .fg(side_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>12.2}", trade.price),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                format!("  {:>10.4}", trade.qty),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_sparkline(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(format!(
            " Throughput (msgs/sec) — peak: {} ",
            format_number(state.peak_rate)
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let sparkline = Sparkline::default()
        .block(block)
        .data(&state.throughput_history)
        .style(Style::default().fg(Color::Green));

    frame.render_widget(sparkline, area);
}

fn render_controls(frame: &mut Frame, area: Rect, state: &TuiState, params: &SharedParams) {
    let block = Block::default()
        .title(" Controls ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let paused = params.paused.load(Ordering::Relaxed);
    let status_style = if paused {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };
    let status_text = if paused {
        "■ PAUSED "
    } else {
        "● RUNNING"
    };

    // Build fragment limit display with selection indicator
    let frag_display: Vec<Span> = FRAGMENT_PRESETS
        .iter()
        .enumerate()
        .flat_map(|(i, &v)| {
            let is_selected = i == state.fragment_idx;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let prefix = if is_selected { "[" } else { " " };
            let suffix = if is_selected { "]" } else { " " };
            vec![
                Span::raw(prefix),
                Span::styled(format!("{}", v), style),
                Span::raw(suffix),
            ]
        })
        .collect();

    let sleep_val = SLEEP_PRESETS[state.sleep_idx];
    let sleep_display = if sleep_val == 0 {
        "0 (none)".to_string()
    } else if sleep_val >= 1000 {
        format!("{}ms", sleep_val / 1000)
    } else {
        format!("{}μs", sleep_val)
    };

    let lines = vec![
        Line::from(vec![]),
        Line::from(vec![
            Span::styled(" [P] ", Style::default().fg(Color::Cyan)),
            Span::raw("Status:    "),
            Span::styled(status_text, status_style),
        ]),
        Line::from(vec![]),
        Line::from(
            std::iter::once(Span::styled(
                " [1-5] ",
                Style::default().fg(Color::Cyan),
            ))
            .chain(std::iter::once(Span::raw("Fragment: ")))
            .chain(frag_display)
            .collect::<Vec<_>>(),
        ),
        Line::from(vec![
            Span::styled("   [S] ", Style::default().fg(Color::Cyan)),
            Span::raw("Sleep:     "),
            Span::styled(
                sleep_display,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("   [I] ", Style::default().fg(Color::Cyan)),
            Span::raw("Idle:      "),
            Span::styled(
                IDLE_NAMES[state.idle_idx],
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![]),
        Line::from(vec![
            Span::styled("   [R] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Reset  "),
            Span::styled("   [Q] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Quit"),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_stats(frame: &mut Frame, area: Rect, state: &TuiState, params: &SharedParams) {
    let block = Block::default()
        .title(" Stats ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let total = params.total_messages.load(Ordering::Relaxed);
    let elapsed = state.start_time.elapsed();
    let uptime = format!("{}m {:02}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60);

    let avg_rate = if elapsed.as_secs() > 0 {
        total / elapsed.as_secs()
    } else {
        0
    };

    // Rate color based on performance
    let rate_color = if state.current_rate > 500_000 {
        Color::Green
    } else if state.current_rate > 100_000 {
        Color::Yellow
    } else {
        Color::Red
    };

    let lines = vec![
        Line::from(vec![]),
        Line::from(vec![
            Span::styled(" Rate:     ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{} msgs/sec", format_number(state.current_rate)),
                Style::default()
                    .fg(rate_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Peak:     ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{} msgs/sec", format_number(state.peak_rate)),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Avg:      ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{} msgs/sec", format_number(avg_rate)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Total:    ", Style::default().fg(Color::White)),
            Span::styled(
                format_number(total),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Uptime:   ", Style::default().fg(Color::White)),
            Span::styled(uptime, Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

// =============================================================================
// MAIN
// =============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Shared state
    let params = Arc::new(SharedParams::new());
    let params_worker = Arc::clone(&params);

    // Channel for trade messages (bounded, non-blocking on send)
    let (trade_tx, trade_rx): (Sender<TradeMsg>, Receiver<TradeMsg>) = bounded(1024);

    // Start worker thread
    let worker = std::thread::Builder::new()
        .name("aeron-poll".into())
        .spawn(move || worker_thread(params_worker, trade_tx))?;

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut state = TuiState::new();

    // Main TUI loop (~30fps)
    let tick_rate = Duration::from_millis(33);

    while params.running.load(Ordering::Relaxed) {
        // Drain trade messages
        while let Ok(trade) = trade_rx.try_recv() {
            state.ticker.push(trade);
            if state.ticker.len() > TICKER_SIZE {
                state.ticker.remove(0);
            }
        }

        // Update throughput every second
        if state.last_tick.elapsed() >= Duration::from_secs(1) {
            let total = params.total_messages.load(Ordering::Relaxed);
            let rate = total - state.last_total;
            state.current_rate = rate;
            if rate > state.peak_rate {
                state.peak_rate = rate;
            }
            state.last_total = total;
            state.last_tick = Instant::now();

            // Shift sparkline history
            state.throughput_history.remove(0);
            state.throughput_history.push(rate);
        }

        // Render
        terminal.draw(|frame| render(frame, &state, &params))?;

        // Handle input
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        params.running.store(false, Ordering::SeqCst);
                    }
                    KeyCode::Char('1') => {
                        state.fragment_idx = 0;
                        params
                            .fragment_limit
                            .store(FRAGMENT_PRESETS[0], Ordering::Relaxed);
                    }
                    KeyCode::Char('2') => {
                        state.fragment_idx = 1;
                        params
                            .fragment_limit
                            .store(FRAGMENT_PRESETS[1], Ordering::Relaxed);
                    }
                    KeyCode::Char('3') => {
                        state.fragment_idx = 2;
                        params
                            .fragment_limit
                            .store(FRAGMENT_PRESETS[2], Ordering::Relaxed);
                    }
                    KeyCode::Char('4') => {
                        state.fragment_idx = 3;
                        params
                            .fragment_limit
                            .store(FRAGMENT_PRESETS[3], Ordering::Relaxed);
                    }
                    KeyCode::Char('5') => {
                        state.fragment_idx = 4;
                        params
                            .fragment_limit
                            .store(FRAGMENT_PRESETS[4], Ordering::Relaxed);
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        state.sleep_idx = (state.sleep_idx + 1) % SLEEP_PRESETS.len();
                        params
                            .sleep_us
                            .store(SLEEP_PRESETS[state.sleep_idx], Ordering::Relaxed);
                    }
                    KeyCode::Char('i') | KeyCode::Char('I') => {
                        state.idle_idx = (state.idle_idx + 1) % IDLE_NAMES.len();
                        params
                            .idle_strategy
                            .store(state.idle_idx as u32, Ordering::Relaxed);
                    }
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        let was_paused = params.paused.load(Ordering::Relaxed);
                        params.paused.store(!was_paused, Ordering::SeqCst);
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        state.throughput_history = vec![0; SPARKLINE_HISTORY];
                        state.peak_rate = 0;
                        state.current_rate = 0;
                        state.last_total = params.total_messages.load(Ordering::Relaxed);
                        state.start_time = Instant::now();
                        state.last_tick = Instant::now();
                    }
                    _ => {}
                }
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    // Wait for worker
    params.running.store(false, Ordering::SeqCst);
    let _ = worker.join();

    let total = params.total_messages.load(Ordering::Relaxed);
    println!("\n=== TUI Subscriber shutdown ===");
    println!("Total messages received: {}", total);

    Ok(())
}
