package com.crypto.marketdata;

import static com.crypto.marketdata.MarketDataMessage.*;

/**
 * Example usage of MarketDataMessage with constants (no casting needed!)
 */
public class MessageExample {

    public static void main(String[] args) {
        // Create a new message
        MarketDataMessage msg = new MarketDataMessage();

        // Encode a message - no (byte) casts needed!
        msg.setTimestamp(System.nanoTime())
           .setSymbol("MOON")
           .setPriceFromDouble(35000.50, EXPONENT_2)      // 2 decimal places
           .setQuantityFromDouble(0.5, EXPONENT_3)        // 3 decimal places
           .setVolumeFromDouble(17500.25, EXPONENT_2)
           .setIsBuyerMaker(true);

        // Print the message
        System.out.println(msg);

        // Decode fields
        System.out.println("\nDecoded fields:");
        System.out.println("  Symbol: " + msg.getSymbol());
        System.out.println("  Price: " + msg.getPrice());
        System.out.println("  Quantity: " + msg.getQuantity());
        System.out.println("  Volume: " + msg.getVolume());
        System.out.println("  Is Buyer Maker: " + msg.isBuyerMaker());

        // Access raw mantissa/exponent (for integer-only math)
        System.out.println("\nRaw encoding:");
        System.out.println("  Price mantissa: " + msg.getPriceMantissa());
        System.out.println("  Price exponent: " + msg.getPriceExponent());

        // Example with different symbols and precisions
        System.out.println("\n--- Meme Token Examples ---");

        String[] symbols = {"LAMBO", "REKT", "HODL", "WAGMI", "TENDIE", "GUH"};
        double[] prices = {42069.69, 0.00001, 1337.80, 420.420, 69.69, 0.1};
        byte[] exponents = {EXPONENT_2, EXPONENT_5, EXPONENT_2, EXPONENT_3, EXPONENT_2, EXPONENT_1};

        for (int i = 0; i < symbols.length; i++) {
            msg.setSymbol(symbols[i])
               .setPriceFromDouble(prices[i], exponents[i])
               .setQuantityFromDouble(Math.random() * 100, EXPONENT_3);

            System.out.printf("%s: %.8f (mantissa=%d, exp=%d)%n",
                msg.getSymbol(),
                msg.getPrice(),
                msg.getPriceMantissa(),
                msg.getPriceExponent()
            );
        }
    }
}
