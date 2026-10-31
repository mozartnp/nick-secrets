# shellcheck shell=bash
#
# assert.sh: the assert_* helpers shared by every test suite in this repository
# (auto_scrum/tests/test_auto_scrum.sh, lib/tests/test_config.sh). Each assert prints
# "  OK: <description>" or "  FAIL: <description> (...)" — the OK count is the suite's
# baseline, so the printed text must not change.
# Contract with the suite that loads it: the asserts increment the global failures, which
# the suite initializes (failures=0) before the first assert and checks at the end.
# Like the libraries in lib/, it only defines functions when loaded: no shebang, no
# executable bit and no set -e of its own. Being a test helper and not a tool library, it
# doesn't follow the config_/CONFIG_ prefix rule of lib/config.sh.

# assert_eq <expected> <actual> <description>
assert_eq() {
  local expected="$1" actual="$2" description="$3"
  if [ "$expected" = "$actual" ]; then
    echo "  OK: $description"
  else
    echo "  FAIL: $description (esperado: '$expected', obtido: '$actual')"
    failures=$((failures + 1))
  fi
}

# assert_contains <haystack> <needle> <description>
assert_contains() {
  local haystack="$1" needle="$2" description="$3"
  case "$haystack" in
    *"$needle"*)
      echo "  OK: $description"
      ;;
    *)
      echo "  FAIL: $description (esperado conter: '$needle')"
      failures=$((failures + 1))
      ;;
  esac
}

# assert_not_contains <haystack> <needle> <description>
assert_not_contains() {
  local haystack="$1" needle="$2" description="$3"
  case "$haystack" in
    *"$needle"*)
      echo "  FAIL: $description (não deveria conter: '$needle')"
      failures=$((failures + 1))
      ;;
    *)
      echo "  OK: $description"
      ;;
  esac
}

# assert_exists <path> <description>
assert_exists() {
  local path="$1" description="$2"
  if [ -e "$path" ]; then
    echo "  OK: $description"
  else
    echo "  FAIL: $description (não existe: '$path')"
    failures=$((failures + 1))
  fi
}

# assert_not_exists <path> <description>
assert_not_exists() {
  local path="$1" description="$2"
  if [ -e "$path" ]; then
    echo "  FAIL: $description (não deveria existir: '$path')"
    failures=$((failures + 1))
  else
    echo "  OK: $description"
  fi
}
