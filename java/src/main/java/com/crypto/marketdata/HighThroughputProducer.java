package com.crypto.marketdata;

import io.aeron.Aeron;
import io.aeron.Publication;
import org.agrona.concurrent.BackoffIdleStrategy;
import org.agrona.concurrent.IdleStrategy;
import org.agrona.concurrent.UnsafeBuffer;

import java.nio.ByteBuffer;
import java.util.Random;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.locks.LockSupport;

import static com.crypto.marketdata.AeronConfig.*;
import static com.crypto.marketdata.MarketDataMessage.*;

/**
 * High-throughput producer with pre-allocated messages.
 * <p>
 * Optimizations:
 * - Pre-generates 10K messages (no allocation in hot path)
 * - Only updates timestamp and sequence before send
 * - Zero GC pressure during publishing
 * - Configurable rate limiting or max throughput
 */
public class HighThroughputProducer {

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

    private static final Random random = new Random(42); // Fixed seed for reproducibility

    // Pre-allocated message buffers (10K messages)
    private static final int MESSAGE_POOL_SIZE = 10_000;
    private static final UnsafeBuffer[] messageBuffers = new UnsafeBuffer[MESSAGE_POOL_SIZE];

    // Statistics
    private static long totalSent = 0;
    private static long backpressureCount = 0;
    private static long notConnectedCount = 0;

    public static void main(String[] args) throws InterruptedException {
        System.out.println("=== High-Throughput Market Data Producer ===\n");

        // Configuration
        final long messagesToSend = Long.MAX_VALUE;  // Run continuously (Ctrl+C to stop)
        final long targetRate = 10_000;              // 10K messages/second (if rate limited)
        final boolean rateLimited = false;           // false = max throughput

        System.out.println("Configuration:");
        System.out.println("  Messages to send: " + messagesToSend);
        System.out.println("  Target rate: " + targetRate + " msgs/sec");
        System.out.println("  Rate limited: " + rateLimited);
        System.out.println();

        // Pre-generate all messages
        System.out.print("Pre-generating " + MESSAGE_POOL_SIZE + " messages... ");
        pregenerateMessages();
        System.out.println("✓ Complete\n");

        // Connect to existing Media Driver
        System.out.println("Connecting to existing Media Driver...");
        final Aeron aeron = connectToMediaDriver();

        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            System.out.println("\nShutting down Producer...");
            aeron.close();
            System.out.println("Aeron client closed");
        }));

        // Create publication
        System.out.println("Creating publication on " + IPC_CHANNEL + " stream " + STREAM_ID);
        final Publication publication = aeron.addPublication(IPC_CHANNEL, STREAM_ID);

        // Wait for subscriber (startup only — Thread.sleep is fine here)
        System.out.print("Waiting for subscriber to connect... ");
        while (!publication.isConnected()) {
            Thread.sleep(10);
        }
        System.out.println("✓ Subscriber connected!\n");

        System.out.println("--- Publishing " + messagesToSend + " messages ---\n");

        // Calculate delay for rate limiting
        final long nanosPerMessage = rateLimited ? (1_000_000_000L / targetRate) : 0;

        // Backpressure idle strategy: 100 spins → 10 yields → park 1μs → park 100μs.
        // This is how Aeron's own samples handle backpressure — spin first (zero cost),
        // then yield, then park at microsecond granularity (1000x finer than Thread.sleep).
        final IdleStrategy offerIdle = new BackoffIdleStrategy(
                100, 10, TimeUnit.MICROSECONDS.toNanos(1), TimeUnit.MICROSECONDS.toNanos(100));

        // Publishing loop
        final long startTime = System.nanoTime();
        long nextSendTime = startTime;

        for (long i = 0; i < messagesToSend; i++) {
            // Get pre-allocated buffer (round-robin)
            final int bufferIndex = (int) (i % MESSAGE_POOL_SIZE);
            final UnsafeBuffer buffer = messageBuffers[bufferIndex];

            // Update only timestamp and sequence (hot path - no allocations!)
            // Use epoch nanos for cross-process latency measurement (Java + Rust)
            long epochNanos = System.currentTimeMillis() * 1_000_000L;
            buffer.putLong(0, epochNanos);            // Update timestamp
            buffer.putInt(RESERVED_OFFSET, (int) i);               // Update sequence

            // Offer message with retry on backpressure
            long result;
            while ((result = publication.offer(buffer, 0, MESSAGE_SIZE)) < 0) {
                if (result == Publication.BACK_PRESSURED) {
                    backpressureCount++;
                    offerIdle.idle(); // spin → yield → parkNanos(1μs..100μs)
                } else if (result == Publication.NOT_CONNECTED) {
                    notConnectedCount++;
                    System.err.println("Warning: Not connected at message " + i);
                    Thread.sleep(10); // rare error path, Thread.sleep is fine
                    break;
                } else {
                    System.err.println("Error offering message " + i + ": " + result);
                    break;
                }
            }

            offerIdle.reset(); // next backpressure starts from spinning again

            if (result > 0) {
                totalSent++;

                // Print progress every 10K messages
                if ((i + 1) % 10_000 == 0) {
                    long elapsed = System.nanoTime() - startTime;
                    double rate = (totalSent * 1_000_000_000.0) / elapsed;
                    System.out.printf("[%,d] Rate: %,.0f msgs/sec  Backpressure: %,d%n",
                            totalSent, rate, backpressureCount);
                }
            }

            // Rate limiting (if enabled)
            if (rateLimited && nanosPerMessage > 0) {
                nextSendTime += nanosPerMessage;
                long now = System.nanoTime();
                long sleepNanos = nextSendTime - now;
                if (sleepNanos > 0) {
                    // Busy spin for very short delays, sleep for longer ones
                    if (sleepNanos < 10_000) {
                        while (System.nanoTime() < nextSendTime) {
                            // Busy spin
                        }
                    } else {
                        LockSupport.parkNanos(sleepNanos);
                    }
                }
            }
        }

        // Final statistics
        final long endTime = System.nanoTime();
        final long totalTimeNanos = endTime - startTime;
        final double totalTimeSec = totalTimeNanos / 1_000_000_000.0;
        final double actualRate = totalSent / totalTimeSec;

        System.out.println("\n--- Publishing Complete ---");
        System.out.println("Total messages sent: " + totalSent);
        System.out.println("Total time: " + String.format("%.3f", totalTimeSec) + " seconds");
        System.out.println("Actual rate: " + String.format("%,.0f", actualRate) + " msgs/sec");
        System.out.println("Backpressure events: " + backpressureCount);
        System.out.println("Not connected events: " + notConnectedCount);

        Thread.sleep(1000); // let final messages drain (shutdown, not hot path)
    }

    /**
     * Pre-generate all messages to avoid allocation during publishing.
     */
    private static void pregenerateMessages() {
        for (int i = 0; i < MESSAGE_POOL_SIZE; i++) {
            // Allocate buffer
            ByteBuffer byteBuffer = ByteBuffer.allocateDirect(MESSAGE_SIZE);
            UnsafeBuffer buffer = new UnsafeBuffer(byteBuffer);

            // Create message wrapper
            MarketDataMessage msg = new MarketDataMessage(buffer, 0);

            // Select random symbol
            String symbol = SYMBOLS[i % SYMBOLS.length];

            // Generate random market data (varies by symbol for realism)
            double price = generatePrice(symbol);
            double quantity = 0.1 + random.nextDouble() * 99.9;  // 0.1 to 100
            double volume = price * quantity;
            boolean isBuyerMaker = random.nextBoolean();

            // Encode message (everything except timestamp and sequence)
            msg.setTimestamp(0)  // Will be updated before send
                    .setSymbol(symbol)
                    .setPriceFromDouble(price, EXPONENT_2)
                    .setQuantityFromDouble(quantity, EXPONENT_3)
                    .setVolumeFromDouble(volume, EXPONENT_2)
                    .setIsBuyerMaker(isBuyerMaker);

            // Sequence will be updated before send
            buffer.putInt(RESERVED_OFFSET, 0);

            messageBuffers[i] = buffer;
        }
    }

    private static double generatePrice(String symbol) {
        return switch (symbol) {
            case "MOON", "LAMBO" -> 30000 + random.nextDouble() * 20000;
            case "DOGE", "SHIB" -> 0.5 + random.nextDouble() * 2.0;
            case "REKT", "NGMI" -> random.nextDouble() * 0.01;
            default -> 1.0 + random.nextDouble() * 1000.0;
        };
    }
}
