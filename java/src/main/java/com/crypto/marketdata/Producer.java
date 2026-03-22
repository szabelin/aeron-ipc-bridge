package com.crypto.marketdata;

import io.aeron.Aeron;
import io.aeron.Publication;
import org.agrona.concurrent.UnsafeBuffer;

import java.nio.ByteBuffer;
import java.util.Random;

import static com.crypto.marketdata.AeronConfig.*;
import static com.crypto.marketdata.MarketDataMessage.*;

/**
 * Simple market data producer.
 *
 * Generates synthetic crypto market data messages and publishes via Aeron IPC.
 * For now: publishes a small number of messages for testing.
 */
public class Producer {

    // Meme token symbols (max 8 chars each)
    private static final String[] SYMBOLS = {
        "MOON", "LAMBO", "REKT", "HODL", "PUMP", "DUMP", "DOGE", "SHIB",
        "PEPE", "WOJAK", "CHAD", "COPE", "HOPE", "NGMI", "WAGMI", "FOMO",
        "YOLO", "DEGEN", "APES", "SAFE", "DIAMOND", "PAPER", "ROCKET", "BEAR",
        "BULL", "WHALE", "SHRIMP", "BAG", "SHILL", "FUD", "ATH", "BTD",
        "SEND", "RIP", "MOON2", "COPE2", "EXIT", "LONG", "SHORT", "FLIP",
        "HYPE", "PAIN", "GAIN", "LOSS", "WIN", "FAIL", "BOOM", "BUST",
        "SQUAD", "CREW", "GANG", "FAM", "FRENS", "ANON", "BASED", "CRINGE",
        "MEME", "YEET", "BRRRR", "GUH", "STONK", "TENDIE", "BAGZ", "RAMEN"
    };

    private static final Random random = new Random();

    // Message counter for sequence numbers (int to fit in 4 reserved bytes)
    private static int messageSequence = 0;

    public static void main(String[] args) throws InterruptedException {
        System.out.println("=== Market Data Producer ===\n");

        // Configuration
        final int messagesToSend = 50;  // Start small for testing
        final long delayBetweenMessages = 10;  // milliseconds

        // Connect to existing Media Driver (started by Consumer)
        System.out.println("Connecting to existing Media Driver...");
        final Aeron aeron = connectToMediaDriver();

        // Setup shutdown hook for Aeron client
        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            System.out.println("\nShutting down Producer...");
            aeron.close();
            System.out.println("Aeron client closed");
        }));

        // Create publication
        System.out.println("Creating publication on " + IPC_CHANNEL + " stream " + STREAM_ID);
        final Publication publication = aeron.addPublication(IPC_CHANNEL, STREAM_ID);

        // Wait for publication to be ready (needs at least one subscriber for IPC)
        System.out.println("Waiting for subscriber to connect...");
        System.out.println("(Start a consumer in another terminal to receive messages)");

        while (!publication.isConnected()) {
            Thread.sleep(100);
        }
        System.out.println("✓ Subscriber connected!\n");

        // Pre-allocate buffer for messages (reuse for zero allocation)
        final UnsafeBuffer buffer = new UnsafeBuffer(ByteBuffer.allocateDirect(MESSAGE_SIZE));
        final MarketDataMessage msg = new MarketDataMessage(buffer, 0);

        // Publish messages
        System.out.println("--- Publishing " + messagesToSend + " messages ---\n");

        int successCount = 0;
        int backpressureCount = 0;

        for (int i = 0; i < messagesToSend; i++) {
            // Generate random market data
            String symbol = SYMBOLS[random.nextInt(SYMBOLS.length)];
            double price = generateRandomPrice(symbol);
            double quantity = random.nextDouble() * 100.0;  // 0-100 units
            double volume = price * quantity;
            boolean isBuyerMaker = random.nextBoolean();

            // Encode message
            msg.setTimestamp(System.nanoTime())
               .setSymbol(symbol)
               .setPriceFromDouble(price, EXPONENT_2)
               .setQuantityFromDouble(quantity, EXPONENT_3)
               .setVolumeFromDouble(volume, EXPONENT_2)
               .setIsBuyerMaker(isBuyerMaker);

            // Use reserved bytes for sequence number (4 bytes only)
            buffer.putInt(RESERVED_OFFSET, messageSequence);

            // Offer message to Aeron
            long result = publication.offer(buffer, 0, MESSAGE_SIZE);

            if (result > 0) {
                messageSequence++;  // Only increment after successful send
                successCount++;
                System.out.printf("[%3d] %-8s  Price: %10.2f  Qty: %8.3f  Vol: %12.2f  %s%n",
                    i + 1, symbol, price, quantity, volume, isBuyerMaker ? "BUY" : "SELL");
            } else if (result == Publication.BACK_PRESSURED) {
                backpressureCount++;
                System.err.println("[" + (i + 1) + "] ✗ Back pressured, retrying...");
                i--;  // Retry this message
                Thread.sleep(1);
            } else if (result == Publication.NOT_CONNECTED) {
                System.err.println("[" + (i + 1) + "] ✗ Not connected!");
                break;
            } else {
                System.err.println("[" + (i + 1) + "] ✗ Failed with code: " + result);
            }

            // Small delay between messages (remove later for high throughput)
            if (delayBetweenMessages > 0) {
                Thread.sleep(delayBetweenMessages);
            }
        }

        // Print statistics
        System.out.println("\n--- Producer Statistics ---");
        System.out.println("Messages sent: " + successCount + "/" + messagesToSend);
        System.out.println("Backpressure events: " + backpressureCount);
        System.out.println("Publication position: " + publication.position());
        System.out.println("Publication max position: " + publication.maxPossiblePosition());

        System.out.println("\nProducer finished. Press Ctrl+C to exit.");

        // Keep running so consumer can catch up
        Thread.sleep(Long.MAX_VALUE);
    }

    private static double generateRandomPrice(String symbol) {
        return switch (symbol) {
            case "MOON", "LAMBO" -> 10000 + random.nextDouble() * 50000;
            case "REKT", "GUH", "RAMEN" -> random.nextDouble() * 0.01;
            case "DOGE", "SHIB", "PEPE" -> random.nextDouble() * 1.0;
            default -> 100 + random.nextDouble() * 900;
        };
    }
}
