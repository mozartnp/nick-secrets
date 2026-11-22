#!/usr/bin/env bash
#
# install.sh: the local installer of the nick (NS-52), called by make install and make
# uninstall (the Makefile only calls it). It installs the binary in $PREFIX/bin/nick and the files
# it runs (bin/nick-agent, bin/nick-init, lib/, auto_scrum/prompt.sh and the templates) in
# $PREFIX/share/nick/, in the same tree as the checkout, so no script changes how it finds lib/ or
# the templates. The nick knows it is installed by the manifest $PREFIX/share/nick/.nick-install
# beside its binary (nick/src/resources.rs). The default prefix is $HOME/.local: no sudo, no
# password. The uninstall removes only what the manifest lists (never rm -r) and never touches the
# state directory (the database, the agents and the logs). Its suite is
# install/tests/test_install.sh.
#
#   install.sh install [--prefix <dir>] [--binary <file>]
#   install.sh uninstall [--prefix <dir>]
#
# Without --binary, the install builds the binary with cargo (--binary is for the tests).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MARKER_NAME=".nick-install"

usage() {
  echo "Uso: install.sh install [--prefix <dir>] [--binary <arquivo>]" >&2
  echo "     install.sh uninstall [--prefix <dir>]" >&2
}

die() {
  echo "$1" >&2
  exit 1
}

# share_files: the files of the checkout that go to share/nick/, relative to the root, one per
# line: every regular file directly in bin/ (not bin/tests/), in each lib/<dir>/ but lib/tests/,
# auto_scrum/prompt.sh and auto_scrum/templates/*.md. A rule, not a list: a new lib or template
# goes in by itself. auto_scrum/auto_scrum.sh stays out (the old tool runs from the checkout).
share_files() {
  local file dir
  for file in "$ROOT_DIR"/bin/*; do
    if [ -f "$file" ]; then
      echo "bin/${file##*/}"
    fi
  done
  for dir in "$ROOT_DIR"/lib/*/; do
    dir="${dir%/}"
    if [ "${dir##*/}" = "tests" ]; then
      continue
    fi
    for file in "$dir"/*; do
      if [ -f "$file" ]; then
        echo "lib/${dir##*/}/${file##*/}"
      fi
    done
  done
  echo "auto_scrum/prompt.sh"
  for file in "$ROOT_DIR"/auto_scrum/templates/*.md; do
    if [ -f "$file" ]; then
      echo "auto_scrum/templates/${file##*/}"
    fi
  done
}

# state_dir: where the nick keeps its database and logs, as the nick computes it
# (nick/src/paths.rs): $XDG_STATE_HOME/nick when absolute, else $HOME/.local/state/nick; empty
# when neither.
state_dir() {
  case "${XDG_STATE_HOME:-}" in
    /*)
      echo "$XDG_STATE_HOME/nick"
      return 0
      ;;
  esac
  if [ -n "${HOME:-}" ]; then
    echo "$HOME/.local/state/nick"
  fi
}

# manifest_entry_valid <line>: 0 when <line> is a path this installer writes (bin/nick or under
# share/nick/, without any ..): a changed manifest must never remove a file outside the prefix.
manifest_entry_valid() {
  case "$1" in
    bin/nick) return 0 ;;
    */../* | */.. | ../*) return 1 ;;
    share/nick/?*) return 0 ;;
  esac
  return 1
}

# remove_installed <prefix> <quiet>: removes every file of the manifest of <prefix> (the binary
# first, the manifest last), then the directories of share/nick/ that became empty, from the
# bottom up. A directory that stays because of something the nick did not install (a file, or a
# folder the manifest does not have) is named in a warning (none when <quiet> is 1: the install
# over an installation reuses them); a directory that stays only because of a kept subdirectory
# of the manifest is not, its subdirectory is. Every line is checked before anything is removed.
remove_installed() {
  local prefix="$1" quiet="$2" manifest line entries=() dirs=() dir entry known
  manifest="$prefix/share/nick/$MARKER_NAME"
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      "" | "#"*) continue ;;
    esac
    if ! manifest_entry_valid "$line"; then
      die "Erro: o manifesto $manifest tem uma linha que não é do nick: $line. Nada foi removido."
    fi
    entries+=("$line")
  done < "$manifest"
  rm -f "$prefix/bin/nick"
  for line in "${entries[@]}"; do
    if [ "$line" != "bin/nick" ] && [ "$line" != "share/nick/$MARKER_NAME" ]; then
      rm -f "$prefix/$line"
    fi
  done
  rm -f "$manifest"
  # The directories under share/nick/ (share/nick/ included), deepest first.
  for line in "${entries[@]}"; do
    dir="${line%/*}"
    while [ "$dir" != "share" ] && [ "$dir" != "bin" ] && [ "$dir" != "$line" ]; do
      dirs+=("$dir")
      # Never loops: a path without a / left (only a bad manifest could have one) stops here.
      if [ "${dir%/*}" = "$dir" ]; then
        break
      fi
      dir="${dir%/*}"
    done
  done
  # The directories of the manifest, one per line between newlines (to look one up).
  known="$(printf '\n%s' "${dirs[@]}")"$'\n'
  while IFS= read -r dir; do
    if [ -z "$dir" ] || [ ! -d "$prefix/$dir" ]; then
      continue
    fi
    if rmdir "$prefix/$dir" 2> /dev/null; then
      continue
    fi
    if [ "$quiet" -eq 1 ]; then
      continue
    fi
    while IFS= read -r entry; do
      entry="${entry#"$prefix/"}"
      case "$known" in
        *$'\n'"$entry"$'\n'*) ;;
        *)
          echo "Mantive $prefix/$dir: tem arquivos que não foram instalados pelo nick."
          break
          ;;
      esac
    done < <(find "$prefix/$dir" -mindepth 1 -maxdepth 1)
  done < <(printf '%s\n' "${dirs[@]}" | LC_ALL=C sort -u -r)
}

# build_binary: builds the nick with cargo and prints the path of the binary.
build_binary() {
  local target
  if ! command -v cargo > /dev/null 2>&1; then
    die "Erro: o cargo não foi encontrado no PATH. O make install precisa dele: instale o Rust (rustup) e rode de novo."
  fi
  target="${CARGO_TARGET_DIR:-$ROOT_DIR/nick/target}"
  case "$target" in
    /*) ;;
    *) die "Erro: CARGO_TARGET_DIR precisa ser um caminho absoluto: $target." ;;
  esac
  (cd "$ROOT_DIR" && cargo build --release --locked --manifest-path "$ROOT_DIR/nick/Cargo.toml") >&2 \
    || die "Erro: o cargo build do nick falhou. Nada foi instalado."
  echo "$target/release/nick"
}

# path_hint <bin dir>: how to put <bin dir> in the PATH, when it is not there.
path_hint() {
  local bin="$1"
  case ":$PATH:" in
    *":$bin:"* | *":$bin/:"*) return 0 ;;
  esac
  echo
  echo "Aviso: $bin não está no PATH. Para abrir o nick de qualquer terminal:"
  # shellcheck disable=SC2016
  # The $PATH is the literal text the operator copies, not this script's PATH.
  printf '  bash/zsh: acrescente ao ~/.bashrc (ou ao ~/.zshrc) a linha\n    export PATH="%s:$PATH"\n' "$bin"
  echo "  fish: rode"
  echo "    fish_add_path $bin"
  echo "  e abra um terminal novo."
}

do_install() {
  local prefix="$1" binary="$2" version share file mode files=()
  if [ -z "$binary" ]; then
    binary="$(build_binary)"
  fi
  if [ ! -f "$binary" ]; then
    die "Erro: o binário do nick não existe: $binary."
  fi
  # Its version heads the manifest: exactly one line starting with "nick " (anything else, as a
  # line of a path, would make the manifest refuse the uninstall).
  version="$("$binary" --version 2> /dev/null)" || version=""
  case "$version" in
    "nick "*$'\n'* | "nick ") version="" ;;
    "nick "*) ;;
    *) version="" ;;
  esac
  if [ -z "$version" ]; then
    die "Erro: o binário $binary não respondeu ao --version como um nick (uma linha começando com 'nick '). Nada foi instalado."
  fi
  share="$prefix/share/nick"
  if [ -f "$share/$MARKER_NAME" ]; then
    remove_installed "$prefix" 1
  fi
  mapfile -t files < <(share_files)
  for file in "${files[@]}"; do
    mode=0644
    case "$file" in
      bin/*) mode=0755 ;;
    esac
    install -D -m "$mode" "$ROOT_DIR/$file" "$share/$file"
  done
  {
    echo "# $version"
    echo "bin/nick"
    echo "share/nick/$MARKER_NAME"
    for file in "${files[@]}"; do
      echo "share/nick/$file"
    done
  } > "$share/$MARKER_NAME.tmp"
  install -m 0644 "$share/$MARKER_NAME.tmp" "$share/$MARKER_NAME"
  rm -f "$share/$MARKER_NAME.tmp"
  # The binary last: it never exists without its files.
  install -D -m 0755 "$binary" "$prefix/bin/nick"
  echo "nick instalado: $prefix/bin/nick ($version)."
  path_hint "$prefix/bin"
}

do_uninstall() {
  local prefix="$1" state
  if [ ! -f "$prefix/share/nick/$MARKER_NAME" ]; then
    echo "Não há instalação do nick em $prefix. Nada foi feito."
    return 0
  fi
  remove_installed "$prefix" 0
  state="$(state_dir)"
  if [ -n "$state" ]; then
    echo "nick desinstalado de $prefix. O banco e os logs em $state foram mantidos."
  else
    echo "nick desinstalado de $prefix. O banco e os logs do nick foram mantidos."
  fi
}

main() {
  local command="${1:-}" prefix="" prefix_set=0 binary=""
  case "$command" in
    install | uninstall) shift ;;
    *)
      usage
      exit 1
      ;;
  esac
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --prefix)
        [ "$#" -ge 2 ] || { usage; exit 1; }
        prefix="$2"
        prefix_set=1
        shift 2
        ;;
      --binary)
        if [ "$command" != "install" ] || [ "$#" -lt 2 ]; then
          usage
          exit 1
        fi
        binary="$2"
        shift 2
        ;;
      *)
        usage
        exit 1
        ;;
    esac
  done
  if [ "$prefix_set" -eq 0 ]; then
    if [ -z "${HOME:-}" ]; then
      die "Erro: sem HOME e sem --prefix, não sei onde instalar o nick."
    fi
    prefix="$HOME/.local"
  fi
  case "$prefix" in
    /*) ;;
    *) die "Erro: o prefixo precisa ser um caminho absoluto: $prefix." ;;
  esac
  # Without the slashes at the end (the root stays /).
  while [ "$prefix" != "/" ] && [ "${prefix%/}" != "$prefix" ]; do
    prefix="${prefix%/}"
  done
  if [ "$command" = "install" ]; then
    do_install "$prefix" "$binary"
  else
    do_uninstall "$prefix"
  fi
}

main "$@"
