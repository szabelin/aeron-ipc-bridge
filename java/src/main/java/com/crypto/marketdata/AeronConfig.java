package com.crypto.marketdata;

import io.aeron.Aeron;
import io.aeron.driver.MediaDriver;
import io.aeron.driver.ThreadingMode;
import org.agrona.concurrent.BusySpinIdleStrategy;

import java.io.File;

/**
 * Aeron Media Driver and client configuration.
 *
 * Sets up:
 * - Embedded Media Driver (runs in same JVM)
 * - IPC transport (shared memory, kernel bypass)
 * - Optimized for M1/M2 MacBook performance
 * - 64 MB term buffers (holds ~1.4M messages of 48 bytes)
 */
public class AeronConfig {

    // IPC channel for local communication (fastest option)
    public static final String IPC_CHANNEL = "aeron:ipc";

    // ==========================================================================
    // CROSS-LANGUAGE BRIDGE CONFIGURATION
    // Java and Rust use the same stream ID for interoperability
    // ==========================================================================

    /** Stream ID for cross-language communication (matches Rust). */
    public static final int STREAM_ID = 1001;

    /** Maximum fragments to poll per iteration. Higher = better throughput. */
    public static final int FRAGMENT_LIMIT = 10;

    // Aeron directory (shared memory location)
    public static final String AERON_DIR = "/tmp/aeron-bridge";

    // Term buffer size: 64 MB (default is 16 MB)
    // With 48-byte messages: 64MB / 48 = ~1.4M messages per buffer
    private static final int TERM_BUFFER_LENGTH = 64 * 1024 * 1024;

    // File page size for memory-mapped files (standard 4KB page)
    private static final int FILE_PAGE_SIZE = 4 * 1024; // 4 KB

    /**
     * Create and start an embedded Media Driver with optimized settings.
     *
     * Configuration:
     * - Threading: SHARED (producer, consumer, conductor on same thread - simpler)
     * - IPC Term Buffer: 64 MB (can hold ~1.4M messages before backpressure)
     * - Directory: /tmp/aeron-bridge (shared with Rust subscriber)
     * - Idle Strategy: BusySpinIdleStrategy (matches Rust for fair comparison)
     *
     * WARNING: BusySpinIdleStrategy uses 100% CPU. For production use,
     * consider BackoffIdleStrategy for CPU efficiency.
     *
     * @return MediaDriver instance (must be closed on shutdown)
     */
    public static MediaDriver startEmbeddedMediaDriver() {
        final MediaDriver.Context driverContext = new MediaDriver.Context()
            // Shared memory directory (Rust consumer will connect here)
            .aeronDirectoryName(AERON_DIR)

            // IPC-specific settings
            .ipcTermBufferLength(TERM_BUFFER_LENGTH)
            .publicationTermBufferLength(TERM_BUFFER_LENGTH)

            // Memory-mapped file settings
            .filePageSize(FILE_PAGE_SIZE)

            // Threading mode: SHARED = simpler, good for demos
            // For max performance: DEDICATED (separate threads for sender/receiver/conductor)
            .threadingMode(ThreadingMode.SHARED)

            // Idle strategy: busy-spin for lowest latency (matches Rust)
            // WARNING: Uses 100% CPU. For production, consider BackoffIdleStrategy.
            .sharedIdleStrategy(new BusySpinIdleStrategy())

            // Error handling - no printStackTrace (causes jitter from stack walking)
            .errorHandler(throwable -> {
                System.err.println("Media Driver Error: " + throwable.getClass().getSimpleName() + ": " + throwable.getMessage());
            })

            // Don't delete on start if directory exists (allows reconnection)
            .dirDeleteOnStart(false)

            // Don't delete on shutdown (allows manual cleanup, prevents breaking reconnects)
            .dirDeleteOnShutdown(false);

        System.out.println("Starting Aeron Media Driver...");
        System.out.println("  Directory: " + AERON_DIR);
        System.out.println("  IPC Term Buffer: " + (TERM_BUFFER_LENGTH / 1024 / 1024) + " MB");
        System.out.println("  Threading Mode: " + driverContext.threadingMode());

        return MediaDriver.launch(driverContext);
    }

    /**
     * Create Aeron client connected to the Media Driver.
     *
     * @return Aeron instance (must be closed on shutdown)
     * @throws IllegalStateException if Media Driver is not running
     */
    public static Aeron connectToMediaDriver() {
        // Check if Media Driver is running
        final File aeronDir = new File(AERON_DIR);
        final File cncFile = new File(aeronDir, "cnc.dat");

        if (!aeronDir.exists() || !cncFile.exists()) {
            System.err.println("\n*** ERROR: Media Driver is not running! ***");
            System.err.println("Aeron directory not found: " + AERON_DIR);
            System.err.println("\nStart the Media Driver first:");
            System.err.println("  cd java && ./gradlew runMediaDriver\n");
            throw new IllegalStateException("Media Driver not running. Start it first with: ./gradlew runMediaDriver");
        }

        final Aeron.Context clientContext = new Aeron.Context()
            .aeronDirectoryName(AERON_DIR);

        System.out.println("Connecting Aeron client to Media Driver at: " + AERON_DIR);

        return Aeron.connect(clientContext);
    }
}
