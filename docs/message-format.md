# Custom Binary Message Format

## Overview

This project uses a custom binary encoding format inspired by **SBE (Simple Binary Encoding)** used by Binance and other exchanges. The format is optimized for:

- ✅ **Zero-copy parsing** with fixed offsets
- ✅ **Integer-based decimal encoding** (no floating-point errors)
- ✅ **Predictable 48-byte size** (cache-friendly)
- ✅ **High-frequency trading performance**

## Message Layout

**Total Size: 48 bytes (fixed)**

| Offset | Field            | Type  | Size | Description                                    |
|--------|------------------|-------|------|------------------------------------------------|
| 0-7    | timestamp        | long  | 8    | Nanosecond timestamp                           |
| 8-15   | symbol           | ASCII | 8    | Trading pair (space-padded, e.g., "MOON    ") |
| 16-23  | priceMantissa    | long  | 8    | Price mantissa (integer part)                  |
| 24-31  | qtyMantissa      | long  | 8    | Quantity mantissa (integer part)               |
| 32-39  | volumeMantissa   | long  | 8    | Volume mantissa (integer part)                 |
| 40     | priceExponent    | byte  | 1    | Price scale (e.g., -2 for 2 decimals)         |
| 41     | qtyExponent      | byte  | 1    | Quantity scale (e.g., -3 for 3 decimals)      |
| 42     | volumeExponent   | byte  | 1    | Volume scale                                   |
| 43     | flags            | byte  | 1    | Metadata (bit 0: isBuyerMaker)                |
| 44-47  | reserved         | -     | 4    | Reserved for future use                        |

## Mantissa + Exponent Encoding

**Formula**: `Real Value = mantissa × 10^exponent`

This is the same approach used by SBE/FIX protocols for financial data.

### Examples

**Price: 35000.50**
```
mantissa = 3500050
exponent = -2
value = 3500050 × 10^-2 = 35000.50
```

**Quantity: 0.5**
```
mantissa = 500
exponent = -3
value = 500 × 10^-3 = 0.5
```

**Price: 42069.69420**
```
mantissa = 4206969420
exponent = -5
value = 4206969420 × 10^-5 = 42069.69420
```

## Why Not Floating-Point?

❌ **Floating-point issues**:
- `0.1 + 0.2 != 0.3` in binary floating-point
- Precision loss in financial calculations
- Non-deterministic rounding

✅ **Integer mantissa benefits**:
- Exact decimal representation
- Faster integer arithmetic
- Predictable behavior across systems
- Industry standard (used by exchanges)

## Why Negative Exponents?

**Negative exponents** follow the standard scientific notation: `value = mantissa × 10^exponent`

**Examples:**
```
EXPONENT_2 = -2   →  3500050 × 10^-2 = 35000.50   (2 decimal places)
EXPONENT_3 = -3   →  500 × 10^-3 = 0.5            (3 decimal places)
EXPONENT_8 = -8   →  100000000 × 10^-8 = 1.0      (Satoshi precision)
```

**Benefits:**
- ✅ Matches SBE/Binance/FIX standards
- ✅ Consistent with scientific notation
- ✅ Can use positive exponents for large integers: `35 × 10^3 = 35000`
- ✅ Simple formula with no division

**Alternative (NOT used):** Positive = decimal places
```
value = mantissa / 10^exponent  ← requires division, non-standard
```

## Usage Examples

### Encoding (Java Producer)

```java
import static com.crypto.marketdata.MarketDataMessage.*;

MarketDataMessage msg = new MarketDataMessage();

msg.setTimestamp(System.nanoTime())
   .setSymbol("MOON")
   .setPriceFromDouble(35000.50, EXPONENT_2)     // 2 decimal places - no cast needed!
   .setQuantityFromDouble(0.5, EXPONENT_3)       // 3 decimal places
   .setVolumeFromDouble(17500.25, EXPONENT_2)
   .setIsBuyerMaker(true);

// Or use raw mantissa values for zero allocation:
msg.setPrice(3500050, EXPONENT_2)      // 35000.50
   .setQuantity(500, EXPONENT_3);      // 0.5

// Publish via Aeron
aeronPublication.offer(msg.getBuffer(), msg.getOffset(), MESSAGE_SIZE);
```

### Decoding (Java/Rust Consumer)

**Java (zero-copy)**:
```java
// Direct buffer access, no allocation
long timestamp = MarketDataMessage.getTimestamp(buffer, offset);
String symbol = MarketDataMessage.getSymbol(buffer, offset);
double price = MarketDataMessage.getPrice(buffer, offset);

// Or access raw mantissa/exponent for integer math
long priceMantissa = MarketDataMessage.getPriceMantissa(buffer, offset);
byte priceExponent = MarketDataMessage.getPriceExponent(buffer, offset);
```

**Rust (zero-copy)**:
```rust
// Read directly from byte slice
let timestamp = i64::from_le_bytes(buffer[0..8].try_into()?);
let symbol = std::str::from_utf8(&buffer[8..16])?.trim();
let price_mantissa = i64::from_le_bytes(buffer[16..24].try_into()?);
let price_exponent = buffer[40] as i8;
let price = price_mantissa as f64 * 10_f64.powi(price_exponent as i32);
```

## Meme Token Symbols

All symbols fit perfectly in 8 bytes:

```
MOON     LAMBO    REKT     HODL     PUMP     DUMP
DOGE     SHIB     PEPE     WOJAK    CHAD     COPE
HOPE     NGMI     WAGMI    FOMO     YOLO     DEGEN
APES     SAFE     DIAMOND  PAPER    ROCKET   BEAR
BULL     WHALE    SHRIMP   BAG      SHILL    FUD
ATH      BTD      SEND     RIP      MOON2    COPE2
EXIT     LONG     SHORT    FLIP     HYPE     PAIN
GAIN     LOSS     WIN      FAIL     BOOM     BUST
SQUAD    CREW     GANG     FAM      FRENS    ANON
BASED    CRINGE   MEME     YEET     BRRRR    GUH
STONK    TENDIE   BAGZ     RAMEN
```

## Performance Characteristics

- **Message size**: 48 bytes (fits in single cache line)
- **Parsing**: Zero-copy, fixed offsets (no branching)
- **Encoding**: Direct buffer writes (no allocations)
- **Throughput**: Target 10M msgs/sec on commodity hardware
- **Latency**: Sub-microsecond parsing (p99 target)

## Comparison to Other Formats

| Format         | Size (bytes) | Parse Speed | Precision | Schema Evolution |
|----------------|--------------|-------------|-----------|------------------|
| JSON           | ~120-200     | Slow        | Lossy     | Easy             |
| Protobuf       | ~40-60       | Medium      | Lossy     | Good             |
| This (Custom)  | 48 (fixed)   | Fast        | Exact     | Manual           |
| Real SBE       | Variable     | Fastest     | Exact     | Good             |

Our custom format trades schema evolution flexibility for simplicity and predictable performance.

## Future Extensions

Reserved 4 bytes (offset 44-47) can be used for:
- Trade ID
- Exchange ID
- Sequence number
- Additional flags
- Checksum/CRC
