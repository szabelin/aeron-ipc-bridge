package com.crypto.marketdata;

import io.aeron.driver.MediaDriver;

import static com.crypto.marketdata.AeronConfig.*;

/**
 * Standalone Media Driver launcher.
 *
 * Run this first, then start either Java Consumer OR Rust Consumer (not both).
 * This ensures clean benchmarking without overlapping consumers.
 *
 * Usage:
 *   ./gradlew runMediaDriver
 *
 * Then in separate terminals:
 *   ./gradlew runConsumer              # Java
 *   cargo run --release --bin consumer # Rust
 */
public class MediaDriverLauncher {

    public static void main(String[] args) {
        System.out.println("=== Standalone Aeron Media Driver ===\n");
        System.out.println("Press Ctrl+C to stop\n");

        final MediaDriver driver = startEmbeddedMediaDriver();

        // Keep running until interrupted
        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            System.out.println("\nShutting down Media Driver...");
            driver.close();
            System.out.println("Media Driver stopped");
        }));

        System.out.println("\nMedia Driver running. Ready for connections.");
        System.out.println("Start consumer and producer in separate terminals.\n");

        // Block forever
        try {
            Thread.currentThread().join();
        } catch (InterruptedException e) {
            // Exit on interrupt
        }
    }
}
