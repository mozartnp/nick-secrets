# shellcheck shell=bash
#
# config.sh: reads the project config file (.nick.conf in the cwd, or any other file the
# caller picks) shared by the nick tools that run an AI agent — today only auto_scrum. The
# tool registers its own keys (config_register_key) and this library does the rest: the
# KEY="value" reader with no source/eval (config_load), its three warnings, the toggle rule
# (*_ENABLED), the per-key default and note, where the cwd file lives (config_cwd_file) and
# the skeleton format (config_print_skeleton). It knows no concrete key of any tool.
#
# Rules (see ABOUT.md, "Código compartilhado entre ferramentas (lib/)"):
#   - loading it only assigns CONFIG_CWD_FILE_NAME, creates the four empty registry arrays
#     and defines functions: it prints nothing and runs no command;
#   - no shebang and no executable bit; no set -euo pipefail of its own, but it works with
#     or without it in the caller;
#   - every function is prefixed config_ and every global CONFIG_;
#   - it never calls exit, and never uses source, . or eval;
#   - it reads no global of the caller: everything comes in as a parameter or from the
#     registry. $PWD (config_cwd_file) and the readonly attribute checked when registering
#     (config_register_key) are shell state, not caller globals.
# The one documented exception: config_load writes, through printf -v, the globals of the
# keys the caller registered — that is the reader's job. It writes nothing else, neither
# another caller global nor a CONFIG_* one. Registering only writes to the CONFIG_* arrays
# and never initializes the key globals: the caller owns them.

CONFIG_CWD_FILE_NAME=".nick.conf"

# The key registry, filled by config_register_key in registration order (which is the order
# of the skeleton and of the missing-key warning). Parallel indexed arrays rather than an
# associative one: declare -A would need -g if this library were loaded inside a function.
# CONFIG_KEYS is the public, read-only list; the other three are internal.
CONFIG_KEYS=()
CONFIG_KEY_DEFAULTS=()
CONFIG_KEY_NOTES=()
CONFIG_KEY_COMMENTS=()

# config_register_key <name> <default> <note> <comment>: registers one key of the caller's
# config, in skeleton order. <default> is the value the skeleton writes and the missing-key
# warning suggests; <note> is the key-specific text both warnings show (may be empty);
# <comment> is the skeleton comment above the key, without the "# " of each line
# (config_print_skeleton adds it). Everything is checked before registering; on any broken
# rule it prints a message on stderr and returns 1 without registering anything (and
# without exit — a caller under set -e stops right there, on purpose: a bad registration is
# a programming error):
#   - <name> matches ^[A-Z][A-Z0-9_]*$. Every key is uppercase and every local of this
#     library is lowercase, so the printf -v in config_load can't hit a local, and a name
#     like a[$(cmd)] never reaches printf -v. The letters are spelled out instead of A-Z, so
#     the check doesn't depend on the locale's collation;
#   - <name> doesn't start with CONFIG_ (this library's namespace) and isn't registered yet;
#   - <name> isn't a special shell variable (BASH*, PATH, IFS, ENV, HOME, CDPATH, PS[0-9],
#     SHELLOPTS, LD_*, LC_*, LANG, TERM, EDITOR, SHELL, GLOBIGNORE, PROMPT_COMMAND, ...) nor a
#     variable that is already readonly. The config file may be edited by another team, so a
#     key with one of those names would let it change how the caller's shell runs commands
#     (PATH="/evil", BASH_ENV, LD_PRELOAD), and printf -v on a readonly variable would stop a
#     caller under set -e — config_load never exits;
#   - <default> is "true" or "false" for a toggle key (config_is_toggle_key) and "" for a
#     text key;
#   - <comment> isn't empty and no line of it ends in a space, a tab or a \r, so the
#     skeleton stays clean for the target team's text hooks (trailing-whitespace,
#     mixed-line-ending).
config_register_key() {
  local upper="ABCDEFGHIJKLMNOPQRSTUVWXYZ"
  local word="ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_"
  local name default note comment line attributes
  if [ "$#" -ne 4 ]; then
    echo "Erro: config_register_key espera 4 argumentos (nome, padrão, nota e comentário), recebeu $#." >&2
    return 1
  fi
  name="$1"
  default="$2"
  note="$3"
  comment="$4"
  case "$name" in
    "" | [!"$upper"]* | *[!"$word"]*)
      echo "Erro: config_register_key: nome de chave inválido '$name' (use só A-Z, 0-9 e _, começando por letra maiúscula)." >&2
      return 1
      ;;
    CONFIG_*)
      echo "Erro: config_register_key: a chave '$name' usa o prefixo CONFIG_, reservado para a biblioteca." >&2
      return 1
      ;;
    BASH* | PATH | IFS | ENV | HOME | CDPATH | PS[0-9] | SHELLOPTS | LD_* | LC_* | LANG | \
      TERM | EDITOR | VISUAL | SHELL | GLOBIGNORE | PROMPT_COMMAND | POSIXLY_CORRECT | PWD | \
      OLDPWD | OPTIND | OPTARG | TMPDIR | HISTFILE | MAIL | MAILPATH | FIGNORE | INPUTRC)
      echo "Erro: config_register_key: a chave '$name' é uma variável especial do shell." >&2
      return 1
      ;;
  esac
  attributes="$(declare -p "$name" 2>/dev/null || true)"
  attributes="${attributes#declare -}"
  attributes="${attributes%% *}"
  case "$attributes" in
    *r*)
      echo "Erro: config_register_key: a chave '$name' já é uma variável readonly." >&2
      return 1
      ;;
  esac
  if config_key_index "$name" >/dev/null; then
    echo "Erro: config_register_key: a chave '$name' já foi registrada." >&2
    return 1
  fi
  if config_is_toggle_key "$name"; then
    case "$default" in
      true | false) ;;
      *)
        echo "Erro: config_register_key: a chave liga/desliga '$name' precisa de padrão \"true\" ou \"false\" (recebeu \"$default\")." >&2
        return 1
        ;;
    esac
  elif [ -n "$default" ]; then
    echo "Erro: config_register_key: a chave de texto '$name' precisa de padrão \"\" (recebeu \"$default\")." >&2
    return 1
  fi
  if [ -z "$comment" ]; then
    echo "Erro: config_register_key: a chave '$name' precisa de um comentário para o esqueleto." >&2
    return 1
  fi
  while IFS= read -r line; do
    case "$line" in
      *[[:space:]])
        echo "Erro: config_register_key: o comentário da chave '$name' tem uma linha terminando em espaço, tab ou CR (CRLF)." >&2
        return 1
        ;;
    esac
  done <<< "$comment"
  CONFIG_KEYS+=("$name")
  CONFIG_KEY_DEFAULTS+=("$default")
  CONFIG_KEY_NOTES+=("$note")
  CONFIG_KEY_COMMENTS+=("$comment")
}

# config_key_index <key>: prints the registry index of <key>, or nothing and returns 1 when
# <key> isn't registered.
config_key_index() {
  local key="$1" index
  for index in "${!CONFIG_KEYS[@]}"; do
    if [ "${CONFIG_KEYS[$index]}" = "$key" ]; then
      printf '%s\n' "$index"
      return 0
    fi
  done
  return 1
}

# config_is_toggle_key <key>: succeeds when <key> is a toggle ("true"/"false") key, i.e. its
# name ends in _ENABLED. This is the only place with that rule — the registry, both config_load
# checks and the skeleton defaults go through it — instead of a separate key list, which
# would be one more place to update for every new flag. What each flag does, and which value
# turns it off, belongs to the tool.
config_is_toggle_key() {
  case "$1" in
    *_ENABLED) return 0 ;;
    *) return 1 ;;
  esac
}

# config_key_default <key>: prints the registered default of <key> (without quotes) — the
# value config_print_skeleton writes and the missing-key warning suggests. Returns 1, printing
# nothing, when <key> isn't registered. That never happens in normal use: the warnings call
# it inside $(...) only with registered keys, so the return 1 can't trip a caller's set -e.
config_key_default() {
  local index
  index="$(config_key_index "$1")" || return 1
  printf '%s\n' "${CONFIG_KEY_DEFAULTS[$index]}"
}

# config_key_note <key>: prints the registered note of <key> — the only key-specific text
# of the warnings — or nothing when the key has none (returning 0). Without a note, the
# invalid-value warning ends with "a opção está desligada.". Returns 1 when <key> isn't
# registered, which never happens in normal use (same as config_key_default).
config_key_note() {
  local index
  index="$(config_key_index "$1")" || return 1
  printf '%s\n' "${CONFIG_KEY_NOTES[$index]}"
}

# config_load <file>
# Reads a config file without source/eval: the file may live in the target project's
# repository, where anyone on that team can edit it, so it must never run code in the shell
# of whoever runs the tool. Only lines KEY="value" whose KEY is in CONFIG_KEYS (exact match)
# are applied, and the value is assigned literally via printf -v (no $VAR, $(...), backtick
# or escape expansion). Surrounding spaces/tabs (and the \r of CRLF files) are trimmed first.
# Everything else — comments, blank lines, unknown keys, commands — is ignored; a line for a
# registered key that doesn't follow the format (no quotes, quotes inside the value, trailing
# comment/command, but also the "almost right" `export KEY=...` and `KEY = ...`) is ignored
# with a warning on stderr (config_warn_malformed_line), so a typo doesn't silently turn a
# flag off.
# Two more checks warn on stderr and never stop the run (the function always returns 0):
#   - every key of CONFIG_KEYS must be declared in the file. The missing ones get a single
#     warning after the loop (config_warn_missing_keys), with the lines to add. A malformed
#     line counts as present, since it already gets the format warning; a commented line or
#     a lookalike key (*_ENABLED_OLD, *_ENABLE) doesn't, since the reader never sees the key
#     in them;
#   - an accepted line of a toggle key (config_is_toggle_key) whose value isn't "true",
#     "false" or "" gets one warning per line (config_warn_invalid_toggle). The value is still
#     assigned as is: what each value does is up to the tool.
# This is the documented exception to "reads/writes no caller global": it writes the globals
# of the registered keys, and nothing else. Its locals are lowercase and every key is
# uppercase (config_register_key), so printf -v always reaches the global.
config_load() {
  local file="$1"
  local line="" line_number=0 key value seen_keys=" " missing_keys=()
  while IFS= read -r line || [ -n "$line" ]; do
    line_number=$((line_number + 1))
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    for key in "${CONFIG_KEYS[@]}"; do
      case "$line" in
        "$key"=*)
          seen_keys+="$key "
          ;;
        # Only a space right after the name counts, so a lookalike key such as
        # KEY_EXTRA = "x" stays silently ignored.
        "export $key"=* | "export $key"[[:space:]]* | "$key"[[:space:]]*=*)
          seen_keys+="$key "
          config_warn_malformed_line "$file:$line_number" "$key"
          continue 2
          ;;
        *) continue ;;
      esac
      value="${line#"$key"=}"
      case "$value" in
        \"*\")
          value="${value#\"}"
          value="${value%\"}"
          case "$value" in
            *\"*) ;;
            *)
              printf -v "$key" '%s' "$value"
              if config_is_toggle_key "$key"; then
                case "$value" in
                  true | false | "") ;;
                  *) config_warn_invalid_toggle "$file:$line_number" "$key" "$value" ;;
                esac
              fi
              continue 2
              ;;
          esac
          ;;
      esac
      config_warn_malformed_line "$file:$line_number" "$key"
    done
  done < "$file"
  for key in "${CONFIG_KEYS[@]}"; do
    case "$seen_keys" in
      *" $key "*) ;;
      *) missing_keys+=("$key") ;;
    esac
  done
  if [ "${#missing_keys[@]}" -gt 0 ]; then
    config_warn_missing_keys "$file" "${missing_keys[@]}"
  fi
  return 0
}

# config_warn_malformed_line <file:line_number> <key>: the single warning text for a
# registered-key line that config_load ignores for not following KEY="value". The location
# comes already joined — passing the bare "$file" here makes shellcheck think the function
# writes to the file config_load is reading (SC2094).
config_warn_malformed_line() {
  local location="$1" key="$2"
  echo "Aviso: $location: linha de $key ignorada (formato esperado: $key=\"valor\", sem aspas duplas dentro do valor)" >&2
}

# config_warn_invalid_toggle <file:line_number> <key> <value>: the single warning text for an
# accepted toggle line whose value isn't "true", "false" or "". The value comes from a file
# another team may edit and goes straight to the terminal, so control characters become "?"
# ([:cntrl:], not [:print:], which under the C locale would also mangle the bytes of "não").
# That is only for the text: the variable keeps the raw value. Only toggle keys get here, so
# a text key is never shown — it is free text that may name a client or hint at
# infrastructure, and it is never validated. The text ends with the key note, or with "a
# opção está desligada." when the key has none. The location comes already joined, as in
# config_warn_malformed_line (SC2094).
config_warn_invalid_toggle() {
  local location="$1" key="$2" value="$3" note
  value="${value//[[:cntrl:]]/?}"
  note="$(config_key_note "$key")"
  echo "Aviso: $location: valor \"$value\" inválido para $key (valores aceitos: \"true\", \"false\" ou \"\") — ${note:-a opção está desligada.}" >&2
}

# config_warn_missing_keys <file> <key>...: the single warning for a loaded file that lacks
# some keys — one block, and only its first line starts with "Aviso:". Each missing key gets
# the line to add, with its registered default (the same line config_print_skeleton writes):
# an absent key is empty, and empty works like the default. A key note goes on a comment line
# before its key, not at the end of the line, so the block can be pasted into the file as is
# (a trailing comment would get the format warning). It never suggests the tool's init: that
# overwrites the file and would wipe the values already in it.
config_warn_missing_keys() {
  local file="$1" key note
  shift
  echo "Aviso: $file não tem todas as chaves da configuração — cada chave ausente funciona como o valor sugerido abaixo. Adicione ao arquivo:" >&2
  for key in "$@"; do
    note="$(config_key_note "$key")"
    if [ -n "$note" ]; then
      echo "  # $key: $note" >&2
    fi
    echo "  $key=\"$(config_key_default "$key")\"" >&2
  done
}

# config_cwd_file: prints the path of the config file in the cwd ($PWD/.nick.conf) and always
# returns 0, whether the file exists or not — testing it with [ -f ] is the caller's job.
# Returning 1 for a missing file would make a caller's x="$(config_cwd_file)" stop under
# set -e. Only the cwd itself: it never searches parent directories.
config_cwd_file() {
  printf '%s\n' "$PWD/$CONFIG_CWD_FILE_NAME"
}

# config_print_skeleton <tool_name>: prints (stdout) the config skeleton of the registered
# keys: three header lines naming <tool_name>, then, for each key in registration order, its
# comment with "# " before each line (a blank comment line becomes "#", with no trailing
# space) and KEY="<default>". The file must stay friendly to the target team's generic text
# hooks (pre-commit trailing-whitespace/end-of-file-fixer/mixed-line-ending): no shebang, no
# trailing spaces, LF, ending with a newline — config_register_key refuses comments that
# would break that. printf '%s\n', not echo, so nothing in the text is interpreted.
config_print_skeleton() {
  local tool_name="$1" index line
  printf '%s\n' \
    "# Config do projeto pro $tool_name. Preencha as variáveis abaixo e salve." \
    "# Não é executado: o $tool_name lê só linhas CHAVE=\"valor\" (aspas duplas, sem aspas dentro do valor nem comentário no fim da linha) das chaves abaixo." \
    "# Mantenha todas as chaves abaixo, mesmo as que não usar (o $tool_name avisa quando falta alguma). Nas chaves *_ENABLED, use só \"true\", \"false\" ou \"\"."
  for index in "${!CONFIG_KEYS[@]}"; do
    while IFS= read -r line; do
      if [ -n "$line" ]; then
        printf '%s\n' "# $line"
      else
        printf '%s\n' "#"
      fi
    done <<< "${CONFIG_KEY_COMMENTS[$index]}"
    printf '%s\n' "${CONFIG_KEYS[$index]}=\"${CONFIG_KEY_DEFAULTS[$index]}\""
  done
}
