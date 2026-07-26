// This structure must exactly match the definition in the real ADK Plugin.h.
// When Plugin.h has already been included (PLUGIN_H is defined), skip this
// definition to avoid a redefinition error. Define it for standalone AmiDate.cpp builds.
#pragma once
#include <cstdint>

#ifndef PLUGIN_H

#pragma pack(push, 1)
struct PackedDate
{
    unsigned int IsFuturePad : 1;
    unsigned int Reserved    : 5;
    unsigned int MicroSec    : 10;
    unsigned int MilliSec    : 10;
    unsigned int Second      : 6;

    unsigned int Minute      : 6;
    unsigned int Hour        : 5;
    unsigned int Day         : 5;
    unsigned int Month       : 4;
    unsigned int Year        : 12;
};

union AmiDate
{
    uint64_t Date;
    PackedDate PackDate;
};
#pragma pack(pop)

#endif // !PLUGIN_H

// Populate AmiDate from a Unix timestamp in microseconds (UTC).
// AmiBroker stores local/exchange time according to the database settings. If a
// timezone other than UTC is needed, convert it before calling this function.
void FillAmiDate(int64_t unix_microseconds, AmiDate& dest);
