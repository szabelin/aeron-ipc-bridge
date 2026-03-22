//! Shared configuration and utilities for Aeron Java-Rust bridge.
//!
//! # Cross-Language IPC Demo
//!
//! This library provides shared configuration for cross-language communication
//! between Java and Rust via Aeron IPC:
//!
//! - **Java Producer** publishes market data messages
//! - **Rust Subscriber** receives and processes the messages
//!
//! Both languages use the same stream ID and message format for interoperability.

// =============================================================================
// AERON CONFIGURATION
// =============================================================================

/// Aeron Media Driver directory (must match Java Media Driver).
pub const AERON_DIR: &str = "/tmp/aeron-bridge";

/// IPC channel for shared memory transport.
pub const IPC_CHANNEL: &str = "aeron:ipc";

// =============================================================================
// CROSS-LANGUAGE BRIDGE CONFIGURATION
// =============================================================================

/// Stream ID for cross-language communication (matches Java).
pub const STREAM_ID: i32 = 1001;

/// Message size for cross-language communication.
/// Must match: java/src/main/java/com/crypto/marketdata/MarketDataMessage.java
///
/// Performance considerations:
/// - 48 bytes fits within a single 64-byte CPU cache line
/// - Size increase from 32 to 48 bytes has negligible throughput impact
///   since CPUs fetch entire cache lines anyway
/// - Long-term: size can grow to 64 bytes (full cache line) without
///   crossing cache line boundaries; beyond 64 bytes would require
///   two cache fetches per message
///
/// Layout (matches Java MarketDataMessage exactly):
///   [0-7]   timestamp (i64 nanoseconds)
///   [8-15]  symbol (8 bytes ASCII, space-padded)
///   [16-23] price mantissa (i64)
///   [24-31] quantity mantissa (i64)
///   [32-39] volume mantissa (i64)
///   [40]    price exponent (i8)
///   [41]    quantity exponent (i8)
///   [42]    volume exponent (i8)
///   [43]    flags (u8, bit 0 = isBuyerMaker)
///   [44-47] reserved (4 bytes)
///
/// Note: Uses little-endian byte order (native on x86/ARM architectures).
pub const MESSAGE_SIZE: usize = 48;

/// Maximum fragments to poll per iteration.
/// Higher values improve throughput; lower values reduce latency variance.
pub const FRAGMENT_LIMIT: i32 = 10;

// =============================================================================
// MESSAGE FIELD OFFSETS (matching Java MarketDataMessage)
// =============================================================================

const SYMBOL_OFFSET: usize = 8;
const SYMBOL_LEN: usize = 8;
const PRICE_MANTISSA_OFFSET: usize = 16;
const QTY_MANTISSA_OFFSET: usize = 24;
const VOLUME_MANTISSA_OFFSET: usize = 32;
const PRICE_EXPONENT_OFFSET: usize = 40;
const QTY_EXPONENT_OFFSET: usize = 41;
const VOLUME_EXPONENT_OFFSET: usize = 42;
const FLAGS_OFFSET: usize = 43;

// =============================================================================
// MESSAGE ENCODING/DECODING (zero-copy, HFT-optimized)
// =============================================================================
//
// Design notes:
// - All decode functions require buffer.len() >= MESSAGE_SIZE (48 bytes)
// - Functions panic on invalid input (fail-fast for HFT - corruption = bug)
// - No Result types to avoid branch overhead in hot path
// - Use debug_assert for development-time bounds checking

/// Encode a timestamp (i64 nanoseconds) into the first 8 bytes of a buffer.
/// Uses little-endian byte order for consistency with Rust's native representation.
#[inline]
pub fn encode_timestamp(buffer: &mut [u8], timestamp_ns: i64) {
    buffer[0..8].copy_from_slice(&timestamp_ns.to_le_bytes());
}

/// Decode a timestamp (i64 nanoseconds) from the first 8 bytes of a buffer.
#[inline]
pub fn decode_timestamp(buffer: &[u8]) -> i64 {
    i64::from_le_bytes(buffer[0..8].try_into().expect("buffer too small"))
}

/// Encode a price (f64) into bytes at the specified offset.
/// Uses little-endian byte order.
#[inline]
pub fn encode_price(buffer: &mut [u8], offset: usize, price: f64) {
    buffer[offset..offset + 8].copy_from_slice(&price.to_le_bytes());
}

/// Decode a price (f64) from bytes at the specified offset.
#[inline]
pub fn decode_price(buffer: &[u8], offset: usize) -> f64 {
    f64::from_le_bytes(
        buffer[offset..offset + 8]
            .try_into()
            .expect("buffer too small"),
    )
}

/// Encode a quantity (u64) into bytes at the specified offset.
#[inline]
pub fn encode_quantity(buffer: &mut [u8], offset: usize, quantity: u64) {
    buffer[offset..offset + 8].copy_from_slice(&quantity.to_le_bytes());
}

/// Decode a quantity (u64) from bytes at the specified offset.
#[inline]
pub fn decode_quantity(buffer: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        buffer[offset..offset + 8]
            .try_into()
            .expect("buffer too small"),
    )
}

// =============================================================================
// MARKET DATA MESSAGE DECODING (matches Java MarketDataMessage)
// =============================================================================

/// Decode symbol (8 bytes ASCII) from buffer, trimming trailing spaces.
/// Expects ASCII data; returns empty string for invalid UTF-8.
/// Requires: buffer.len() >= MESSAGE_SIZE (48 bytes)
#[inline]
pub fn decode_symbol(buffer: &[u8]) -> &str {
    debug_assert!(
        buffer.len() >= MESSAGE_SIZE,
        "buffer must be at least MESSAGE_SIZE bytes"
    );
    let bytes = &buffer[SYMBOL_OFFSET..SYMBOL_OFFSET + SYMBOL_LEN];
    core::str::from_utf8(bytes).unwrap_or("").trim_end()
}

/// Decode price mantissa (i64) from buffer.
/// Requires: buffer.len() >= MESSAGE_SIZE
#[inline]
pub fn decode_price_mantissa(buffer: &[u8]) -> i64 {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    i64::from_le_bytes(
        buffer[PRICE_MANTISSA_OFFSET..PRICE_MANTISSA_OFFSET + 8]
            .try_into()
            .unwrap(),
    )
}

/// Decode quantity mantissa (i64) from buffer.
#[inline]
pub fn decode_qty_mantissa(buffer: &[u8]) -> i64 {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    i64::from_le_bytes(
        buffer[QTY_MANTISSA_OFFSET..QTY_MANTISSA_OFFSET + 8]
            .try_into()
            .unwrap(),
    )
}

/// Decode volume mantissa (i64) from buffer.
#[inline]
pub fn decode_volume_mantissa(buffer: &[u8]) -> i64 {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    i64::from_le_bytes(
        buffer[VOLUME_MANTISSA_OFFSET..VOLUME_MANTISSA_OFFSET + 8]
            .try_into()
            .unwrap(),
    )
}

/// Decode price exponent (i8) from buffer.
#[inline]
pub fn decode_price_exponent(buffer: &[u8]) -> i8 {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    buffer[PRICE_EXPONENT_OFFSET] as i8
}

/// Decode quantity exponent (i8) from buffer.
#[inline]
pub fn decode_qty_exponent(buffer: &[u8]) -> i8 {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    buffer[QTY_EXPONENT_OFFSET] as i8
}

/// Decode volume exponent (i8) from buffer.
#[inline]
pub fn decode_volume_exponent(buffer: &[u8]) -> i8 {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    buffer[VOLUME_EXPONENT_OFFSET] as i8
}

/// Decode flags byte from buffer.
#[inline]
pub fn decode_flags(buffer: &[u8]) -> u8 {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    buffer[FLAGS_OFFSET]
}

/// Check if the trade was buyer-maker (bit 0 of flags).
/// Requires: buffer.len() >= MESSAGE_SIZE
#[inline]
pub fn is_buyer_maker(buffer: &[u8]) -> bool {
    debug_assert!(buffer.len() >= MESSAGE_SIZE);
    (buffer[FLAGS_OFFSET] & 0x01) != 0
}

/// Convert mantissa and exponent to f64: mantissa * 10^exponent
#[inline]
pub fn mantissa_to_f64(mantissa: i64, exponent: i8) -> f64 {
    mantissa as f64 * 10_f64.powi(exponent as i32)
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_round_trip() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let original_ts: i64 = 1_234_567_890_123_456_789;

        encode_timestamp(&mut buffer, original_ts);
        let decoded_ts = decode_timestamp(&buffer);

        assert_eq!(original_ts, decoded_ts);
    }

    #[test]
    fn test_timestamp_zero() {
        let mut buffer = [0u8; MESSAGE_SIZE];

        encode_timestamp(&mut buffer, 0);
        assert_eq!(decode_timestamp(&buffer), 0);
    }

    #[test]
    fn test_timestamp_negative() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let negative_ts: i64 = -1_000_000;

        encode_timestamp(&mut buffer, negative_ts);
        assert_eq!(decode_timestamp(&buffer), negative_ts);
    }

    #[test]
    fn test_timestamp_max() {
        let mut buffer = [0u8; MESSAGE_SIZE];

        encode_timestamp(&mut buffer, i64::MAX);
        assert_eq!(decode_timestamp(&buffer), i64::MAX);
    }

    #[test]
    fn test_price_round_trip() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let price: f64 = 35000.50;

        encode_price(&mut buffer, 8, price);
        let decoded_price = decode_price(&buffer, 8);

        assert_eq!(price, decoded_price);
    }

    #[test]
    fn test_price_precision() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let price: f64 = 0.123456789012345;

        encode_price(&mut buffer, 8, price);
        let decoded_price = decode_price(&buffer, 8);

        // f64 should preserve this precision exactly
        assert_eq!(price, decoded_price);
    }

    #[test]
    fn test_price_large_value() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let price: f64 = 999_999_999.99;

        encode_price(&mut buffer, 8, price);
        assert_eq!(decode_price(&buffer, 8), price);
    }

    #[test]
    fn test_quantity_round_trip() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let quantity: u64 = 1_000_000;

        encode_quantity(&mut buffer, 16, quantity);
        let decoded_qty = decode_quantity(&buffer, 16);

        assert_eq!(quantity, decoded_qty);
    }

    #[test]
    fn test_quantity_max() {
        let mut buffer = [0u8; MESSAGE_SIZE];

        encode_quantity(&mut buffer, 16, u64::MAX);
        assert_eq!(decode_quantity(&buffer, 16), u64::MAX);
    }

    #[test]
    fn test_generic_encode_decode() {
        // Test generic offset-based encode/decode functions.
        // Note: These are utility functions, NOT the MarketDataMessage layout.
        // For MarketDataMessage tests, see test_decode_* functions below.
        let mut buffer = [0u8; MESSAGE_SIZE];

        let timestamp: i64 = 1_700_000_000_000_000_000; // ~2023 in nanos
        let price: f64 = 35000.50;
        let quantity: u64 = 100;

        encode_timestamp(&mut buffer, timestamp);
        encode_price(&mut buffer, 8, price);
        encode_quantity(&mut buffer, 16, quantity);

        // Verify encode/decode round-trip at given offsets
        assert_eq!(decode_timestamp(&buffer), timestamp);
        assert_eq!(decode_price(&buffer, 8), price);
        assert_eq!(decode_quantity(&buffer, 16), quantity);
    }

    #[test]
    fn test_message_size_is_48_bytes() {
        // Must match Java MarketDataMessage.MESSAGE_SIZE
        assert_eq!(MESSAGE_SIZE, 48);
    }

    #[test]
    fn test_buffer_independence() {
        // Ensure encoding one field doesn't affect others
        let mut buffer = [0xFFu8; MESSAGE_SIZE];

        encode_timestamp(&mut buffer, 0);

        // Only first 8 bytes should be zeroed
        assert_eq!(&buffer[0..8], &[0u8; 8]);
        assert_eq!(&buffer[8..16], &[0xFFu8; 8]); // Unchanged
    }

    // =========================================================================
    // MARKET DATA MESSAGE DECODE TESTS
    // =========================================================================

    #[test]
    fn test_decode_symbol() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        buffer[8..16].copy_from_slice(b"MOON    ");
        assert_eq!(decode_symbol(&buffer), "MOON");
    }

    #[test]
    fn test_decode_symbol_full_length() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        buffer[8..16].copy_from_slice(b"BTCUSDT!");
        assert_eq!(decode_symbol(&buffer), "BTCUSDT!");
    }

    #[test]
    fn test_decode_price_mantissa() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let mantissa: i64 = 3500050;
        buffer[16..24].copy_from_slice(&mantissa.to_le_bytes());
        assert_eq!(decode_price_mantissa(&buffer), mantissa);
    }

    #[test]
    fn test_decode_qty_mantissa() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let mantissa: i64 = 500;
        buffer[24..32].copy_from_slice(&mantissa.to_le_bytes());
        assert_eq!(decode_qty_mantissa(&buffer), mantissa);
    }

    #[test]
    fn test_decode_volume_mantissa() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        let mantissa: i64 = 1750025;
        buffer[32..40].copy_from_slice(&mantissa.to_le_bytes());
        assert_eq!(decode_volume_mantissa(&buffer), mantissa);
    }

    #[test]
    fn test_decode_exponents() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        buffer[40] = 0xFE; // -2 as i8
        buffer[41] = 0xFD; // -3 as i8
        buffer[42] = 0xFC; // -4 as i8
        assert_eq!(decode_price_exponent(&buffer), -2);
        assert_eq!(decode_qty_exponent(&buffer), -3);
        assert_eq!(decode_volume_exponent(&buffer), -4);
    }

    #[test]
    fn test_mantissa_to_f64() {
        assert_eq!(mantissa_to_f64(3500050, -2), 35000.50);
        assert_eq!(mantissa_to_f64(500, -3), 0.5);
        assert_eq!(mantissa_to_f64(100, 0), 100.0);
    }

    #[test]
    fn test_is_buyer_maker() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        buffer[43] = 0x01;
        assert!(is_buyer_maker(&buffer));
        buffer[43] = 0x00;
        assert!(!is_buyer_maker(&buffer));
        buffer[43] = 0x03; // bit 0 still set
        assert!(is_buyer_maker(&buffer));
    }

    #[test]
    fn test_decode_flags() {
        let mut buffer = [0u8; MESSAGE_SIZE];
        buffer[43] = 0xAB;
        assert_eq!(decode_flags(&buffer), 0xAB);
    }
}
