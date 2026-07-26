// AmiBroker ADK shim. Its only responsibilities are converting structures and calling Rust.
// This file contains no HTTP, WebSocket, JSON, or business logic.
//
// The PluginInfo, RecentInfo, Quotation, and PluginNotification structures below
// match the official ADK. This file includes the real ADK Plugin.h and does not
// redefine those structures.

#include <windows.h>       // Must precede Plugin.h, which uses Windows types and constants.

#include "Plugin.h"        // Official ADK header copied to adk/Include/Plugin.h.
#include "market_core.h"   // C ABI for the Rust core.
#include "AmiDate.h"
#include <unordered_map>
#include <memory>
#include <string>
#include <mutex>
#include <cstdio>

// ---- Plugin metadata: customize for the actual product ----
#define PLUGIN_ID_CODE   0x564E4272   // Any four-character identifier, such as 'VNBr'. 
#define PLUGIN_VERSION   10000        // 1.0.0
#define PLUGIN_NAME      "VNBrokers Data Plugin"
#define PLUGIN_VENDOR    "VNBrokers"
#define PLUGIN_MIN_AMI   6000000      // Requires AmiBroker 6.00 or later.

// ---- Global shim state ----
static MDHandle g_engine = nullptr;
static HWND     g_mainWnd = nullptr;

// RecentInfo needs a stable lifetime because AmiBroker retains the pointer from
// GetRecentInfo. unique_ptr keeps addresses stable even when the map rehashes.
static std::unordered_map<std::string, std::unique_ptr<RecentInfo>> g_recent;
static std::mutex g_recentMutex;

static RecentInfo* GetOrCreateRecentInfo(const std::string& symbol)
{
    std::lock_guard<std::mutex> lock(g_recentMutex);
    auto it = g_recent.find(symbol);
    if (it != g_recent.end())
        return it->second.get();

    auto info = std::make_unique<RecentInfo>();
    ZeroMemory(info.get(), sizeof(RecentInfo));
    info->nStructSize = sizeof(RecentInfo);
    strncpy_s(info->Name, symbol.c_str(), sizeof(info->Name) - 1);

    RecentInfo* ptr = info.get();
    g_recent.emplace(symbol, std::move(info));
    return ptr;
}

// ---- Rust -> C++ callback, running on a Rust runtime thread rather than the main thread ----
// Rust holds no locks while this function runs: cache.update_recent releases its
// RwLock before invoking the callback. This function briefly locks g_recentMutex
// and uses PostMessage to avoid cross-thread blocking and deadlocks.
extern "C" void __cdecl OnMarketDataUpdate(
    void* /*context*/,
    const char* symbol,
    const MDRecent* recent)
{
    if (symbol == nullptr || recent == nullptr)
        return;

    RecentInfo* info = GetOrCreateRecentInfo(symbol);

    {
        std::lock_guard<std::mutex> lock(g_recentMutex);
        info->fLast = recent->last;
        info->fOpen = recent->open;
        info->fHigh = recent->high;
        info->fLow = recent->low;
        info->fPrev = recent->previous_close;
        info->fChange = recent->change;
        info->fBid = recent->bid;
        info->fAsk = recent->ask;
        info->iBidSize = recent->bid_size;
        info->iAskSize = recent->ask_size;
        info->fTradeVol = recent->trade_volume;
        info->fTotalVol = recent->total_volume;
        info->iTradeVol = static_cast<int>(recent->trade_volume);
        info->iTotalVol = static_cast<int>(recent->total_volume);

        info->nBitmap = RI_LAST | RI_OPEN | RI_HIGHLOW | RI_PREVCHANGE |
                        RI_BID | RI_ASK | RI_TRADEVOL | RI_TOTALVOL | RI_DATEUPDATE;
        info->nStatus = RI_STATUS_UPDATE | RI_STATUS_BIDASK | RI_STATUS_TRADE |
                        RI_STATUS_BARSREADY;

        AmiDate ts;
        FillAmiDate(recent->unix_microseconds, ts);
        info->nDateUpdate = ts.PackDate.Year * 10000 + ts.PackDate.Month * 100 + ts.PackDate.Day;
        info->nTimeUpdate = ts.PackDate.Hour * 10000 + ts.PackDate.Minute * 100 + ts.PackDate.Second;
    }

    // PostMessage is safer than SendMessage here. A cross-thread SendMessage
    // would block the Rust thread until AmiBroker's UI thread handled it, which
    // could cause latency or a deadlock during another plugin call.
    if (g_mainWnd != nullptr)
    {
        // WM_USER_STREAMING_UPDATE is defined by the real ADK Plugin.h.
        PostMessageA(g_mainWnd, WM_USER_STREAMING_UPDATE, 0, (LPARAM)info);
    }
}

// ---- ADK exports ----

PLUGINAPI int GetPluginInfo(struct PluginInfo* info)
{
    info->nStructSize = sizeof(PluginInfo);
    info->nType = PLUGIN_TYPE_DATA;
    info->nVersion = PLUGIN_VERSION;
    info->nIDCode = PLUGIN_ID_CODE;
    strncpy_s(info->szName, PLUGIN_NAME, sizeof(info->szName) - 1);
    strncpy_s(info->szVendor, PLUGIN_VENDOR, sizeof(info->szVendor) - 1);
    info->nCertificate = 0;
    info->nMinAmiVersion = PLUGIN_MIN_AMI;
    return 1;
}

PLUGINAPI int Init(void)
{
    int32_t rc = md_create(OnMarketDataUpdate, nullptr, &g_engine);
    if (rc != 0)
        return 0;

    rc = md_start(g_engine);
    return rc == 0 ? 1 : 0;
}

PLUGINAPI int Release(void)
{
    if (g_engine != nullptr)
    {
        md_destroy(g_engine);
        g_engine = nullptr;
    }
    return 1;
}

PLUGINAPI int Notify(struct PluginNotification* pn)
{
    if (pn == nullptr)
        return 0;

    if (pn->hMainWnd != nullptr)
        g_mainWnd = pn->hMainWnd;

    return 1;
}

PLUGINAPI int GetQuotesEx(
    LPCTSTR ticker,
    int periodicity,
    int /*lastValid*/,
    int capacity,
    struct Quotation* quotes,
    GQEContext* /*context*/)
{
    if (g_engine == nullptr || ticker == nullptr || quotes == nullptr || capacity <= 0)
        return 0;

    char symbolUtf8[256];
#ifdef UNICODE
    WideCharToMultiByte(CP_UTF8, 0, ticker, -1, symbolUtf8, sizeof(symbolUtf8), nullptr, nullptr);
#else
    strncpy_s(symbolUtf8, ticker, sizeof(symbolUtf8) - 1);
    symbolUtf8[sizeof(symbolUtf8) - 1] = '\0';
#endif

    // Trigger backfill and realtime subscription for this symbol (idempotent in Rust).
    md_request_symbol(g_engine, symbolUtf8);

    thread_local std::vector<MDBar> buffer;
    buffer.resize(capacity);

    const int32_t periodicitySeconds = periodicity; // ADK uses seconds below daily periodicity.
    const int32_t count = md_copy_bars(
        g_engine,
        symbolUtf8,
        periodicitySeconds,
        buffer.data(),
        capacity
    );

    if (count <= 0)
        return 0;

    for (int32_t i = 0; i < count; ++i)
    {
        const MDBar& source = buffer[i];
        Quotation& destination = quotes[i];

        destination.DateTime.Date = 0;
        FillAmiDate(source.unix_microseconds, destination.DateTime);

        destination.Open = source.open;
        destination.High = source.high;
        destination.Low = source.low;
        destination.Price = source.close;
        destination.Volume = source.volume;
        destination.OpenInterest = source.open_interest;
        destination.AuxData1 = source.aux1;
        destination.AuxData2 = source.aux2;
    }

    return count;
}

// Legacy wrapper used when GetQuotesEx is unavailable or with very old databases.
// Retained for compatibility rather than performance.
PLUGINAPI int GetQuotes(
    LPCTSTR ticker,
    int periodicity,
    int lastValid,
    int size,
    struct QuotationFormat4* quotes)
{
    std::vector<Quotation> buffer(size);
    const int count = GetQuotesEx(ticker, periodicity, lastValid, size, buffer.data(), nullptr);

    for (int i = 0; i < count; ++i)
    {
        ConvertFormat5Quote(&buffer[i], &quotes[i]);
    }
    return count;
}

PLUGINAPI struct RecentInfo* GetRecentInfo(LPCTSTR ticker)
{
    if (g_engine == nullptr || ticker == nullptr)
        return nullptr;

    char symbolUtf8[256];
#ifdef UNICODE
    WideCharToMultiByte(CP_UTF8, 0, ticker, -1, symbolUtf8, sizeof(symbolUtf8), nullptr, nullptr);
#else
    strncpy_s(symbolUtf8, ticker, sizeof(symbolUtf8) - 1);
    symbolUtf8[sizeof(symbolUtf8) - 1] = '\0';
#endif

    RecentInfo* info = GetOrCreateRecentInfo(symbolUtf8);

    // Synchronize once more from cache in case AmiBroker calls GetRecentInfo
    // before the first streaming update, for example immediately after backfill.
    MDRecent recent;
    if (md_copy_recent(g_engine, symbolUtf8, &recent) == 0)
    {
        std::lock_guard<std::mutex> lock(g_recentMutex);
        info->fLast = recent.last;
        info->fOpen = recent.open;
        info->fHigh = recent.high;
        info->fLow = recent.low;
        info->fPrev = recent.previous_close;
        info->fChange = recent.change;
        info->fBid = recent.bid;
        info->fAsk = recent.ask;
        info->iBidSize = recent.bid_size;
        info->iAskSize = recent.ask_size;
        info->fTradeVol = recent.trade_volume;
        info->fTotalVol = recent.total_volume;
        info->nBitmap |= RI_LAST | RI_OPEN | RI_HIGHLOW | RI_PREVCHANGE |
                         RI_BID | RI_ASK | RI_TRADEVOL | RI_TOTALVOL;
    }

    return info;
}

PLUGINAPI int IsBackfillComplete(LPCTSTR ticker)
{
    if (g_engine == nullptr || ticker == nullptr)
        return 0;

    char symbolUtf8[256];
#ifdef UNICODE
    WideCharToMultiByte(CP_UTF8, 0, ticker, -1, symbolUtf8, sizeof(symbolUtf8), nullptr, nullptr);
#else
    strncpy_s(symbolUtf8, ticker, sizeof(symbolUtf8) - 1);
    symbolUtf8[sizeof(symbolUtf8) - 1] = '\0';
#endif
    return md_is_backfill_complete(g_engine, symbolUtf8);
}

PLUGINAPI int GetSymbolLimit(void)
{
    // Limit concurrent WS subscriptions. Adjust this to match server capacity.
    return 2000;
}

PLUGINAPI int SetTimeBase(int /*nTimeBase*/)
{
    // nTimeBase is AmiBroker's preferred default periodicity. Store it if the
    // Rust core needs to prioritize a backfill timeframe.
    return 1;
}

PLUGINAPI int Configure(LPCTSTR /*pszPath*/, struct InfoSite* /*site*/)
{
    // Open a separate configuration tool for JSON settings, endpoints, and API keys.
    // A simple implementation could open the config file or launch a dedicated executable.
    ShellExecuteA(nullptr, "open", "notepad.exe", "vnbrokers-config.json", nullptr, SW_SHOW);
    return 1;
}

PLUGINAPI int GetPluginStatus(struct PluginStatus* status)
{
    if (status == nullptr || g_engine == nullptr)
        return 0;

    status->nStructSize = sizeof(PluginStatus);
    status->nStatusCode = 0; // 0 = OK according to the ADK highest-nibble convention.
    status->clrStatusColor = RGB(0, 128, 0);
    strncpy_s(status->szShortMessage, "Connect OK", sizeof(status->szShortMessage) - 1);
    strncpy_s(status->szLongMessage, "Connecting to localhost:3000/ws and REST API", sizeof(status->szLongMessage) - 1);
    return 1;
}

BOOL APIENTRY DllMain(HMODULE /*module*/, DWORD reason, LPVOID /*reserved*/)
{
    switch (reason)
    {
        case DLL_PROCESS_ATTACH:
        case DLL_THREAD_ATTACH:
        case DLL_THREAD_DETACH:
        case DLL_PROCESS_DETACH:
            break;
    }
    return TRUE;
}
