#pragma once

#include <algorithm>
#include <chrono>
#include <thread>
#include <type_traits>

namespace zeroshm::detail {

/// Spin-wait with exponential backoff until predicate returns true.
/// Predicate may be a pure check or a side-effecting CAS attempt.
template <typename Pred> void spin_wait(Pred &&pred) {
   int backoff_us = 1;
   constexpr int max_backoff_us = 1000;   // Cap at 1ms

   while (!pred()) {
      std::this_thread::sleep_for(std::chrono::microseconds(backoff_us));
      if (backoff_us < max_backoff_us) {
         backoff_us *= 2;
      }
   }
}

/// Spin-wait with exponential backoff and timeout.
/// Returns true if predicate became true, false on timeout.
template <typename Pred, typename Rep, typename Period>
[[nodiscard]] bool
spin_wait_for(Pred &&pred, const std::chrono::duration<Rep, Period> &timeout) {
   auto deadline = std::chrono::steady_clock::now() + timeout;
   long long backoff_us = 1;
   constexpr long long max_backoff_us = 1000;

   while (!pred()) {
      auto now = std::chrono::steady_clock::now();
      if (now >= deadline) {
         return false;
      }
      auto remaining_us =
          std::chrono::duration_cast<std::chrono::microseconds>(deadline - now)
              .count();

      // Use common_type to ensure both arguments to std::min have the same
      // type. remaining_us is microseconds::rep (long long); backoff_us must
      // match.
      using common_t =
          typename std::common_type<long long, decltype(remaining_us)>::type;
      const int sleep_us =
          static_cast<int>(std::min(static_cast<common_t>(backoff_us),
                                    static_cast<common_t>(remaining_us)));
      if (sleep_us > 0) {
         std::this_thread::sleep_for(std::chrono::microseconds(sleep_us));
      }
      if (backoff_us < max_backoff_us) {
         backoff_us *= 2;
      }
   }
   return true;
}

}   // namespace zeroshm::detail
