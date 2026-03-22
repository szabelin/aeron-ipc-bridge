//! Rust subscriber for cross-language IPC demo.
//!
//! Subscribes to messages published by Java Producer via Aeron IPC.

use aeron_java_rust_bridge::{
    decode_price_exponent, decode_price_mantissa, decode_qty_exponent, decode_qty_mantissa,
    decode_symbol, decode_timestamp, is_buyer_maker, mantissa_to_f64, AERON_DIR, FRAGMENT_LIMIT,
    IPC_CHANNEL, MESSAGE_SIZE, STREAM_ID,
};
use rusteron_client::*;
use std::cell::Cell;
use std::error::Error;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Number of messages to print in detail at startup
const DETAILED_MSG_COUNT: u64 = 10;
/// Print summary every N messages after detailed phase
const SUMMARY_INTERVAL: u64 = 1000;

fn main() -> Result<(), Box<dyn Error>> {
    println!("=== Aeron Java-Rust Bridge: Subscriber ===\n");

    // Setup graceful shutdown on Ctrl+C
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nCtrl+C received, shutting down...");
        r.store(false, Ordering::SeqCst);
    })?;

    // Connect to Aeron Media Driver
    println!("Connecting to Media Driver at: {}", AERON_DIR);

    let ctx = AeronContext::new()?;
    let aeron_dir = CString::new(AERON_DIR)?;
    ctx.set_dir(&aeron_dir)?;

    let aeron = Aeron::new(&ctx)?;
    aeron.start()?;
    println!("Connected to Aeron");

    // Create subscription
    println!("Subscribing to {} stream {}", IPC_CHANNEL, STREAM_ID);

    let channel = CString::new(IPC_CHANNEL)?;
    let subscription = aeron
        .async_add_subscription(
            &channel,
            STREAM_ID,
            Handlers::no_available_image_handler(),
            Handlers::no_unavailable_image_handler(),
        )?
        .poll_blocking(Duration::from_secs(10))?;

    println!("Subscription created, waiting for messages...\n");

    // Fragment handler that tracks message count
    struct MessageHandler {
        count: Cell<u64>,
    }

    /// Handles incoming market data messages from Java producer.
    ///
    /// Buffer format: 48 bytes matching Java MarketDataMessage layout.
    /// Decode functions panic on corruption (fail-fast for HFT).
    impl AeronFragmentHandlerCallback for MessageHandler {
        fn handle_aeron_fragment_handler(&mut self, buffer: &[u8], _header: AeronHeader) {
            let count = self.count.get() + 1;
            self.count.set(count);

            // Validate buffer size before decoding
            if buffer.len() < MESSAGE_SIZE {
                eprintln!(
                    "[{}] Error: buffer {} bytes, need {} bytes",
                    count,
                    buffer.len(),
                    MESSAGE_SIZE
                );
                return;
            }

            // Decode all message fields (zero-copy from buffer)
            let timestamp = decode_timestamp(buffer);
            let symbol = decode_symbol(buffer);
            let price =
                mantissa_to_f64(decode_price_mantissa(buffer), decode_price_exponent(buffer));
            let qty = mantissa_to_f64(decode_qty_mantissa(buffer), decode_qty_exponent(buffer));
            let side = if is_buyer_maker(buffer) {
                "BUY"
            } else {
                "SELL"
            };

            // Print first N messages in detail, then summary every M messages
            if count <= DETAILED_MSG_COUNT {
                println!(
                    "[{:>3}] {} | {:>8} | Price: {:>12.2} | Qty: {:>10.4} | {}",
                    count, timestamp, symbol, price, qty, side
                );
            } else if count % SUMMARY_INTERVAL == 0 {
                println!(
                    "[{}] {} {} @ {:.2} (qty: {:.4})",
                    count, symbol, side, price, qty
                );
            }
        }
    }

    let (handler, inner) = Handler::leak_with_fragment_assembler(MessageHandler {
        count: Cell::new(0),
    })?;

    // Main receive loop
    let mut last_count = 0u64;
    let mut last_report = std::time::Instant::now();

    while running.load(Ordering::SeqCst) {
        subscription.poll(Some(&handler), FRAGMENT_LIMIT as usize)?;

        // Print stats every 5 seconds
        let current_count = inner.count.get();
        if last_report.elapsed().as_secs() >= 5 && current_count > last_count {
            println!("--- Stats: {} messages received ---", current_count);
            last_count = current_count;
            last_report = std::time::Instant::now();
        }

        // Small sleep to avoid busy-spinning when no messages
        std::thread::sleep(Duration::from_micros(100));
    }

    println!("\n=== Subscriber shutting down ===");
    println!("Total messages received: {}", inner.count.get());

    Ok(())
}
