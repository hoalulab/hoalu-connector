#include "AmiDate.h"

namespace
{
    // Howard Hinnant's public-domain civil_from_days algorithm converts days
    // since 1970-01-01 into a Gregorian (year, month, day). It works for all
    // years without <ctime>, avoiding 32-bit time_t and OS timezone issues.
    void civil_from_days(
        long long z,
        int& year,
        unsigned& month,
        unsigned& day)
    {
        z += 719468;
        const long long era = (z >= 0 ? z : z - 146096) / 146097;
        const unsigned doe = static_cast<unsigned>(z - era * 146097);
        const unsigned yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        const long long y = static_cast<long long>(yoe) + era * 400;
        const unsigned doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        const unsigned mp = (5 * doy + 2) / 153;
        day = doy - (153 * mp + 2) / 5 + 1;
        month = mp + (mp < 10 ? 3 : -9);
        year = static_cast<int>(y + (month <= 2 ? 1 : 0));
    }
}

void FillAmiDate(int64_t unix_microseconds, AmiDate& dest)
{
    // Normalize to avoid incorrect negative division for dates before 1970.
    long long total_micros = unix_microseconds;
    long long micros_of_day = total_micros % 86'400'000'000LL;
    long long days = total_micros / 86'400'000'000LL;

    if (micros_of_day < 0)
    {
        micros_of_day += 86'400'000'000LL;
        days -= 1;
    }

    int year = 0;
    unsigned month = 0, day = 0;
    civil_from_days(days, year, month, day);

    const long long micros_total = micros_of_day;
    const int hour = static_cast<int>(micros_total / 3'600'000'000LL);
    const long long rem_after_hour = micros_total % 3'600'000'000LL;
    const int minute = static_cast<int>(rem_after_hour / 60'000'000LL);
    const long long rem_after_minute = rem_after_hour % 60'000'000LL;
    const int second = static_cast<int>(rem_after_minute / 1'000'000LL);
    const long long rem_after_second = rem_after_minute % 1'000'000LL;
    const int millisec = static_cast<int>(rem_after_second / 1000LL);
    const int microsec = static_cast<int>(rem_after_second % 1000LL);

    dest.Date = 0;
    dest.PackDate.IsFuturePad = 0;
    dest.PackDate.Reserved    = 0;
    dest.PackDate.MicroSec    = static_cast<unsigned>(microsec) & 0x3FF;
    dest.PackDate.MilliSec    = static_cast<unsigned>(millisec) & 0x3FF;
    dest.PackDate.Second      = static_cast<unsigned>(second) & 0x3F;
    dest.PackDate.Minute      = static_cast<unsigned>(minute) & 0x3F;
    dest.PackDate.Hour        = static_cast<unsigned>(hour) & 0x1F;
    dest.PackDate.Day         = static_cast<unsigned>(day) & 0x1F;
    dest.PackDate.Month       = static_cast<unsigned>(month) & 0xF;
    dest.PackDate.Year        = static_cast<unsigned>(year) & 0xFFF;
}
