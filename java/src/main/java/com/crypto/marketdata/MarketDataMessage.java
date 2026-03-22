package com.crypto.marketdata;

import org.agrona.DirectBuffer;
import org.agrona.MutableDirectBuffer;
import org.agrona.concurrent.UnsafeBuffer;

import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;

/**
 * Custom binary message format for crypto market data.
 *
 * Inspired by SBE (Simple Binary Encoding) used by Binance and other exchanges.
 * Uses mantissa + exponent encoding for decimal precision without floating-point errors.
 *
 * This is a flyweight wrapper around a DirectBuffer - it doesn't own the buffer data,
 * just provides encoding/decoding methods. Can wrap existing buffers to avoid buffer
 * allocation in the encoding hot path.
 *
 * Key design principles:
 * - Fixed offsets for fast encoding/decoding
 * - Integer-based decimal encoding (mantissa × 10^exponent)
 * - Predictable 48-byte message size
 * - Cache-friendly alignment (fits in 64-byte cache line)
 *
 * Message Layout (48 bytes fixed):
 * [0-7]   timestamp       - nanosecond timestamp (long, 8 bytes)
 * [8-15]  symbol          - trading pair symbol (8 bytes ASCII, space-padded, e.g., "MOON    ")
 * [16-23] priceMantissa   - price mantissa (long, 8 bytes)
 * [24-31] qtyMantissa     - quantity mantissa (long, 8 bytes)
 * [32-39] volumeMantissa  - volume mantissa (long, 8 bytes)
 * [40]    priceExponent   - price scale (byte, 1 byte, e.g., -2 for 2 decimals)
 * [41]    qtyExponent     - quantity scale (byte, 1 byte, e.g., -3 for 3 decimals)
 * [42]    volumeExponent  - volume scale (byte, 1 byte)
 * [43]    flags           - metadata flags (byte, 1 byte)
 *                           bit 0: isBuyerMaker (1=buy, 0=sell)
 * [44-47] reserved        - reserved for future use (4 bytes)
 *
 * Example:
 *   Price 35000.50 = mantissa: 3500050, exponent: -2 (3500050 × 10^-2)
 *   Quantity 0.5   = mantissa: 500, exponent: -3 (500 × 10^-3)
 */
public class MarketDataMessage {

    // Message structure constants
    public static final int MESSAGE_SIZE = 48;

    // Common exponent values (negative = decimal places)
    // Usage: mantissa × 10^exponent
    // Example: price 35000.50 = mantissa 3500050, exponent -2 (2 decimal places)
    public static final byte EXPONENT_1 = -1;   // 1 decimal place
    public static final byte EXPONENT_2 = -2;   // 2 decimal places (prices)
    public static final byte EXPONENT_3 = -3;   // 3 decimal places (quantities)
    public static final byte EXPONENT_5 = -5;   // 5 decimal places (high precision)

    // Field offsets
    private static final int TIMESTAMP_OFFSET = 0;
    private static final int SYMBOL_OFFSET = 8;
    private static final int SYMBOL_LENGTH = 8;
    private static final int PRICE_MANTISSA_OFFSET = 16;
    private static final int QTY_MANTISSA_OFFSET = 24;
    private static final int VOLUME_MANTISSA_OFFSET = 32;
    private static final int PRICE_EXPONENT_OFFSET = 40;
    private static final int QTY_EXPONENT_OFFSET = 41;
    private static final int VOLUME_EXPONENT_OFFSET = 42;
    private static final int FLAGS_OFFSET = 43;

    /** Offset of reserved bytes (4 bytes). Can be used for sequence numbers. */
    public static final int RESERVED_OFFSET = 44;

    // Flags bit masks
    private static final byte IS_BUYER_MAKER_MASK = 0x01;

    private final MutableDirectBuffer buffer;
    private final int offset;

    /**
     * Wrap an existing buffer for encoding/decoding.
     */
    public MarketDataMessage(MutableDirectBuffer buffer, int offset) {
        this.buffer = buffer;
        this.offset = offset;
    }

    /**
     * Create a new message with a fresh buffer.
     */
    public MarketDataMessage() {
        this(new UnsafeBuffer(ByteBuffer.allocateDirect(MESSAGE_SIZE)), 0);
    }

    // ========== ENCODER METHODS ==========

    public MarketDataMessage setTimestamp(long timestamp) {
        buffer.putLong(offset + TIMESTAMP_OFFSET, timestamp);
        return this;
    }

    public MarketDataMessage setSymbol(String symbol) {
        // Pad or truncate to exactly 8 bytes
        byte[] symbolBytes = new byte[SYMBOL_LENGTH];
        byte[] input = symbol.getBytes(StandardCharsets.US_ASCII);
        int copyLen = Math.min(input.length, SYMBOL_LENGTH);
        System.arraycopy(input, 0, symbolBytes, 0, copyLen);

        // Pad with spaces if needed
        for (int i = copyLen; i < SYMBOL_LENGTH; i++) {
            symbolBytes[i] = (byte) ' ';
        }

        buffer.putBytes(offset + SYMBOL_OFFSET, symbolBytes);
        return this;
    }

    public MarketDataMessage setPrice(long mantissa, byte exponent) {
        buffer.putLong(offset + PRICE_MANTISSA_OFFSET, mantissa);
        buffer.putByte(offset + PRICE_EXPONENT_OFFSET, exponent);
        return this;
    }

    public MarketDataMessage setPriceFromDouble(double price, byte exponent) {
        long mantissa = (long) (price * Math.pow(10, -exponent));
        return setPrice(mantissa, exponent);
    }

    public MarketDataMessage setQuantity(long mantissa, byte exponent) {
        buffer.putLong(offset + QTY_MANTISSA_OFFSET, mantissa);
        buffer.putByte(offset + QTY_EXPONENT_OFFSET, exponent);
        return this;
    }

    public MarketDataMessage setQuantityFromDouble(double quantity, byte exponent) {
        long mantissa = (long) (quantity * Math.pow(10, -exponent));
        return setQuantity(mantissa, exponent);
    }

    public MarketDataMessage setVolume(long mantissa, byte exponent) {
        buffer.putLong(offset + VOLUME_MANTISSA_OFFSET, mantissa);
        buffer.putByte(offset + VOLUME_EXPONENT_OFFSET, exponent);
        return this;
    }

    public MarketDataMessage setVolumeFromDouble(double volume, byte exponent) {
        long mantissa = (long) (volume * Math.pow(10, -exponent));
        return setVolume(mantissa, exponent);
    }

    public MarketDataMessage setIsBuyerMaker(boolean isBuyerMaker) {
        byte flags = buffer.getByte(offset + FLAGS_OFFSET);
        if (isBuyerMaker) {
            flags |= IS_BUYER_MAKER_MASK;
        } else {
            flags &= ~IS_BUYER_MAKER_MASK;
        }
        buffer.putByte(offset + FLAGS_OFFSET, flags);
        return this;
    }

    // ========== DECODER METHODS (static) ==========

    public static long getTimestamp(DirectBuffer buffer, int offset) {
        return buffer.getLong(offset + TIMESTAMP_OFFSET);
    }

    public static String getSymbol(DirectBuffer buffer, int offset) {
        byte[] symbolBytes = new byte[SYMBOL_LENGTH];
        buffer.getBytes(offset + SYMBOL_OFFSET, symbolBytes);
        return new String(symbolBytes, StandardCharsets.US_ASCII).trim();
    }

    public static long getPriceMantissa(DirectBuffer buffer, int offset) {
        return buffer.getLong(offset + PRICE_MANTISSA_OFFSET);
    }

    public static byte getPriceExponent(DirectBuffer buffer, int offset) {
        return buffer.getByte(offset + PRICE_EXPONENT_OFFSET);
    }

    public static double getPrice(DirectBuffer buffer, int offset) {
        long mantissa = getPriceMantissa(buffer, offset);
        byte exponent = getPriceExponent(buffer, offset);
        return mantissa * Math.pow(10, exponent);
    }

    public static long getQuantityMantissa(DirectBuffer buffer, int offset) {
        return buffer.getLong(offset + QTY_MANTISSA_OFFSET);
    }

    public static byte getQuantityExponent(DirectBuffer buffer, int offset) {
        return buffer.getByte(offset + QTY_EXPONENT_OFFSET);
    }

    public static double getQuantity(DirectBuffer buffer, int offset) {
        long mantissa = getQuantityMantissa(buffer, offset);
        byte exponent = getQuantityExponent(buffer, offset);
        return mantissa * Math.pow(10, exponent);
    }

    public static long getVolumeMantissa(DirectBuffer buffer, int offset) {
        return buffer.getLong(offset + VOLUME_MANTISSA_OFFSET);
    }

    public static byte getVolumeExponent(DirectBuffer buffer, int offset) {
        return buffer.getByte(offset + VOLUME_EXPONENT_OFFSET);
    }

    public static double getVolume(DirectBuffer buffer, int offset) {
        long mantissa = getVolumeMantissa(buffer, offset);
        byte exponent = getVolumeExponent(buffer, offset);
        return mantissa * Math.pow(10, exponent);
    }

    public static boolean isBuyerMaker(DirectBuffer buffer, int offset) {
        byte flags = buffer.getByte(offset + FLAGS_OFFSET);
        return (flags & IS_BUYER_MAKER_MASK) != 0;
    }

    // ========== INSTANCE DECODER METHODS ==========

    public long getTimestamp() {
        return getTimestamp(buffer, offset);
    }

    public String getSymbol() {
        return getSymbol(buffer, offset);
    }

    public long getPriceMantissa() {
        return getPriceMantissa(buffer, offset);
    }

    public byte getPriceExponent() {
        return getPriceExponent(buffer, offset);
    }

    public double getPrice() {
        return getPrice(buffer, offset);
    }

    public double getQuantity() {
        return getQuantity(buffer, offset);
    }

    public double getVolume() {
        return getVolume(buffer, offset);
    }

    public boolean isBuyerMaker() {
        return isBuyerMaker(buffer, offset);
    }

    @Override
    public String toString() {
        return String.format(
            "MarketDataMessage{symbol='%s', price=%.8f, qty=%.8f, volume=%.8f, isBuyerMaker=%b}",
            getSymbol(), getPrice(), getQuantity(), getVolume(), isBuyerMaker()
        );
    }
}
