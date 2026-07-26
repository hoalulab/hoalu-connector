// C ABI boundary between Plugin.cpp (C++ ADK shim) and market_data_core.lib (Rust).
// Must match the #[repr(C)] structures in rust-core/src/ffi.rs exactly.
// Do not add std::string, C++ classes, or any other non-POD types here.
#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void* MDHandle;

typedef struct MDBar
{
    int64_t unix_microseconds;

    float open;
    float high;
    float low;
    float close;

    float volume;
    float open_interest;
    float aux1;
    float aux2;
} MDBar;

typedef struct MDRecent
{
    float last;
    float open;
    float high;
    float low;
    float previous_close;
    float change;
    float change_percent;

    float bid;
    float ask;

    float trade_volume;
    float total_volume;

    int32_t bid_size;
    int32_t ask_size;

    int64_t unix_microseconds;
    int32_t flags;
} MDRecent;

typedef void(__cdecl* MDUpdateCallback)(
    void* context,
    const char* symbol,
    const MDRecent* recent
);

// Create the engine and write its handle to output. Return 0 on success.
int32_t md_create(
    MDUpdateCallback callback,
    void* callback_context,
    MDHandle* output
);

// Start the background runtime (WS + REST). Idempotent.
int32_t md_start(MDHandle handle);

// Stop the background runtime without destroying the handle.
void md_stop(MDHandle handle);

// Stop the runtime if necessary and free the engine. The handle becomes invalid.
void md_destroy(MDHandle handle);

// Request a symbol: trigger REST backfill and subscribe over WS. Idempotent.
int32_t md_request_symbol(
    MDHandle handle,
    const char* symbol
);

// Copy up to `capacity` recent bars, sorted chronologically, into output.
// Return the number of copied bars, or a negative value on error.
int32_t md_copy_bars(
    MDHandle handle,
    const char* symbol,
    int32_t periodicity_seconds,
    MDBar* output,
    int32_t capacity
);

// Copy the current RecentInfo. Return 0 on success or -1 if data is unavailable.
int32_t md_copy_recent(
    MDHandle handle,
    const char* symbol,
    MDRecent* output
);

// Return 1 if REST backfill for the symbol is complete; otherwise return 0.
int32_t md_is_backfill_complete(
    MDHandle handle,
    const char* symbol
);

// Return a pointer to the latest error string. It remains valid until the next
// md_last_error call on the same thread because Rust uses a thread-local buffer.
const char* md_last_error(MDHandle handle);

#ifdef __cplusplus
}
#endif
