package com.crypto.marketdata;

import org.agrona.concurrent.UnsafeBuffer;
import org.junit.jupiter.api.Test;

import java.nio.ByteBuffer;

import static com.crypto.marketdata.MarketDataMessage.*;
import static org.junit.jupiter.api.Assertions.*;

/**
 * Unit tests for MarketDataMessage encoding/decoding.
 */
class MarketDataMessageTest {

    @Test
    void encodeDecodeRoundTrip() {
        UnsafeBuffer buffer = new UnsafeBuffer(ByteBuffer.allocateDirect(MESSAGE_SIZE));
        MarketDataMessage msg = new MarketDataMessage(buffer, 0);

        // Encode
        msg.setTimestamp(123456789L)
           .setSymbol("MOON")
           .setPriceFromDouble(35000.50, EXPONENT_2)
           .setQuantityFromDouble(1.5, EXPONENT_3)
           .setVolumeFromDouble(52500.75, EXPONENT_2)
           .setIsBuyerMaker(true);

        // Decode and verify
        assertEquals("MOON", getSymbol(buffer, 0));
        assertEquals(35000.50, getPrice(buffer, 0), 0.01);
        assertEquals(1.5, getQuantity(buffer, 0), 0.001);
        assertEquals(52500.75, getVolume(buffer, 0), 0.01);
        assertTrue(isBuyerMaker(buffer, 0));
    }

    @Test
    void symbolPaddedToEightBytes() {
        UnsafeBuffer buffer = new UnsafeBuffer(ByteBuffer.allocateDirect(MESSAGE_SIZE));
        MarketDataMessage msg = new MarketDataMessage(buffer, 0);

        msg.setSymbol("BTC");
        assertEquals("BTC", getSymbol(buffer, 0));  // trim() removes padding
    }

    @Test
    void symbolTruncatedIfTooLong() {
        UnsafeBuffer buffer = new UnsafeBuffer(ByteBuffer.allocateDirect(MESSAGE_SIZE));
        MarketDataMessage msg = new MarketDataMessage(buffer, 0);

        msg.setSymbol("VERYLONGSYMBOL");
        assertEquals("VERYLONG", getSymbol(buffer, 0));  // Truncated to 8 chars
    }

    @Test
    void buyerMakerFlag() {
        UnsafeBuffer buffer = new UnsafeBuffer(ByteBuffer.allocateDirect(MESSAGE_SIZE));
        MarketDataMessage msg = new MarketDataMessage(buffer, 0);

        msg.setIsBuyerMaker(true);
        assertTrue(isBuyerMaker(buffer, 0));

        msg.setIsBuyerMaker(false);
        assertFalse(isBuyerMaker(buffer, 0));
    }

    @Test
    void mantissaExponentEncoding() {
        UnsafeBuffer buffer = new UnsafeBuffer(ByteBuffer.allocateDirect(MESSAGE_SIZE));
        MarketDataMessage msg = new MarketDataMessage(buffer, 0);

        // Price: 123.45 with 2 decimal places
        msg.setPrice(12345, EXPONENT_2);
        assertEquals(12345, getPriceMantissa(buffer, 0));
        assertEquals(EXPONENT_2, getPriceExponent(buffer, 0));
        assertEquals(123.45, getPrice(buffer, 0), 0.001);
    }

    @Test
    void instanceDecoderMethods() {
        MarketDataMessage msg = new MarketDataMessage();

        msg.setSymbol("DOGE")
           .setPriceFromDouble(0.42, EXPONENT_2)
           .setQuantityFromDouble(1000.0, EXPONENT_1)
           .setVolumeFromDouble(420.0, EXPONENT_2)
           .setIsBuyerMaker(false);

        // Use instance decoder methods
        assertEquals("DOGE", msg.getSymbol());
        assertEquals(0.42, msg.getPrice(), 0.01);
        assertEquals(1000.0, msg.getQuantity(), 0.1);
        assertEquals(420.0, msg.getVolume(), 0.01);
        assertFalse(msg.isBuyerMaker());
    }

    @Test
    void messageSize() {
        assertEquals(48, MESSAGE_SIZE);
    }
}
