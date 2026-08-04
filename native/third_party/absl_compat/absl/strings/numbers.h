// Minimal absl compatibility shim for the vendored worldline sources
// (Plugin track P.3). The ref build links real Abseil; here we only need the
// two string->number helpers used by classic_args.cpp. Semantics match
// absl::SimpleAtoi/SimpleAtod for well-formed input (optional sign, full
// string consumed). This is a build shim, NOT a general absl replacement.

#ifndef ABSL_STRINGS_NUMBERS_H_
#define ABSL_STRINGS_NUMBERS_H_

#include <cstdlib>
#include <string>
#include <string_view>

namespace absl {

using string_view = std::string_view;

inline bool SimpleAtoi(string_view str, int* out) {
  if (str.empty() || out == nullptr) return false;
  std::string tmp(str);
  char* end = nullptr;
  long value = std::strtol(tmp.c_str(), &end, 10);
  if (end != tmp.c_str() + tmp.size()) return false;
  *out = static_cast<int>(value);
  return true;
}

inline bool SimpleAtod(string_view str, double* out) {
  if (str.empty() || out == nullptr) return false;
  std::string tmp(str);
  char* end = nullptr;
  double value = std::strtod(tmp.c_str(), &end);
  if (end != tmp.c_str() + tmp.size()) return false;
  *out = value;
  return true;
}

}  // namespace absl

#endif  // ABSL_STRINGS_NUMBERS_H_
