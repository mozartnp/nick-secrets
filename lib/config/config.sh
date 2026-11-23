# shellcheck shell=bash
#
# config.sh: the generic part of the nick project config library (lib/config/). It reads the
# project config file (.nick/nick.conf in the cwd, or any other file the caller picks) shared
# by the nick tools — today auto_scrum and bin/nick-init. The keys are registered through
# config_register_key (every key of the nick lives in lib/config/keys.sh,
# config_register_nick_keys) and this file does the rest: the KEY="value" reader with no
# source/eval (config_load), its three warnings, the toggle rule (*_ENABLED), the per-key
# default and note, where the cwd file lives (config_cwd_file, plus the check for the old
# .nick.conf, config_cwd_legacy_check), the skeleton format (config_print_skeleton), the
# .nick/ tree in the cwd (config_dir_prepare) and the init that creates it with the skeleton
# (config_init). This file knows no concrete key and no tool: the tool name comes as a
# parameter.
#
# Rules (see ABOUT.md, "Código compartilhado entre ferramentas (lib/)"):
#   - loading it only assigns CONFIG_NICK_DIR_NAME, CONFIG_EXCHANGE_DIR_NAME,
#     CONFIG_DOCS_DIR_NAME, CONFIG_CWD_FILE_NAME and CONFIG_CWD_LEGACY_FILE_NAME, creates the
#     five empty registry arrays and defines functions: it prints nothing and runs no
#     command;
#   - no shebang and no executable bit; no set -euo pipefail of its own, but it works with
#     or without it in the caller;
#   - every function is prefixed config_ and every global CONFIG_;
#   - it never calls exit, and never uses source, . or eval;
#   - it reads no global of the caller: everything comes in as a parameter or from the
#     registry. $PWD (config_cwd_file, config_cwd_legacy_check, config_dir_prepare,
#     config_init) and the readonly attribute
#     checked when registering (config_register_key) are shell state, not caller globals.
# The one documented exception: config_load writes, through printf -v, the globals of the
# keys the caller registered — that is the reader's job. It writes nothing else, neither
# another caller global nor a CONFIG_* one. Registering only writes to the CONFIG_* arrays
# and never initializes the key globals: the caller owns them.

# All relative to the cwd (the target project). The nick dir holds the project config, the
# exchange dir (where the user and the agents trade files) and the per-ticket docs
# (docs/<ID>/, created by the agent when it writes there). Inside the project, so it stays
# writable with the jail on. Spelled out, not built from each other: a test checks that
# CONFIG_CWD_FILE_NAME is $CONFIG_NICK_DIR_NAME/nick.conf. The legacy name is never read:
# config_cwd_legacy_check only tells the user to move it.
CONFIG_NICK_DIR_NAME=".nick"
CONFIG_EXCHANGE_DIR_NAME=".nick/troca"
CONFIG_DOCS_DIR_NAME=".nick/docs"
CONFIG_CWD_FILE_NAME=".nick/nick.conf"
CONFIG_CWD_LEGACY_FILE_NAME=".nick.conf"

# The key registry, filled by config_register_key in registration order (which is the order
# of the skeleton and of the missing-key warning). Parallel indexed arrays rather than an
# associative one: declare -A would need -g if this library were loaded inside a function.
# CONFIG_KEYS is the public, read-only list; the other four are internal. CONFIG_KEY_PROMPTS
# holds the init question of each key ("" when the key has none, NS-53).
CONFIG_KEYS=()
CONFIG_KEY_DEFAULTS=()
CONFIG_KEY_NOTES=()
CONFIG_KEY_COMMENTS=()
CONFIG_KEY_PROMPTS=()

# config_register_key <name> <default> <note> <comment> [<prompt>]: registers one key of the
# caller's config, in skeleton order. <default> is the value the skeleton writes and the
# missing-key warning suggests; <note> is the key-specific text both warnings show (may be
# empty); <comment> is the skeleton comment above the key, without the "# " of each line
# (config_print_skeleton adds it); <prompt>, optional (NS-53), is the question config_init asks
# for this key, whose answer goes into the skeleton — an empty <prompt> is the same as none.
# Everything is checked before registering; on any broken
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
#     mixed-line-ending);
#   - a toggle key has no <prompt>: its value is "true"/"false", never a typed answer.
config_register_key() {
  local upper="ABCDEFGHIJKLMNOPQRSTUVWXYZ"
  local word="ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_"
  local name default note comment prompt line attributes
  if [ "$#" -ne 4 ] && [ "$#" -ne 5 ]; then
    echo "Erro: config_register_key espera 4 ou 5 argumentos (nome, padrão, nota, comentário e, opcional, a pergunta do init), recebeu $#." >&2
    return 1
  fi
  name="$1"
  default="$2"
  note="$3"
  comment="$4"
  prompt="${5:-}"
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
    if [ -n "$prompt" ]; then
      echo "Erro: config_register_key: a chave liga/desliga '$name' não pode ter pergunta do init." >&2
      return 1
    fi
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
  CONFIG_KEY_PROMPTS+=("$prompt")
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

# config_cwd_file: prints the path of the config file in the cwd ($PWD/.nick/nick.conf) and always
# returns 0, whether the file exists or not — testing it with [ -f ] is the caller's job.
# Returning 1 for a missing file would make a caller's x="$(config_cwd_file)" stop under
# set -e. Only the cwd itself: it never searches parent directories.
config_cwd_file() {
  printf '%s\n' "$PWD/$CONFIG_CWD_FILE_NAME"
}

# config_cwd_legacy_check: looks for the old config file in the cwd ($PWD/.nick.conf), which
# is no longer read. Only the old one → prints the error with the mv that migrates it on
# stderr and returns 1 (stopping is the caller's job: this library never exits). Both → the
# new one wins: prints a warning on stderr (whoever edited the old one by mistake would
# otherwise see no effect) and returns 0. Otherwise silent, returns 0. Never reads, moves,
# creates or removes anything.
config_cwd_legacy_check() {
  local legacy_file current_file
  legacy_file="$PWD/$CONFIG_CWD_LEGACY_FILE_NAME"
  current_file="$(config_cwd_file)"
  if [ ! -e "$legacy_file" ]; then
    return 0
  fi
  if [ -e "$current_file" ]; then
    echo "Aviso: $legacy_file não é mais lido — a configuração vem de $current_file. Apague o arquivo antigo." >&2
    return 0
  fi
  {
    echo "Erro: a configuração do projeto agora fica em $CONFIG_CWD_FILE_NAME, e $legacy_file não é mais lido."
    echo "Mova o arquivo e rode de novo (na raiz do projeto):"
    echo "  mkdir -p $(dirname "$CONFIG_CWD_FILE_NAME") && mv $CONFIG_CWD_LEGACY_FILE_NAME $CONFIG_CWD_FILE_NAME"
    echo "Depois, versione $CONFIG_CWD_FILE_NAME e o $(dirname "$CONFIG_CWD_FILE_NAME")/.gitignore que o nick cria na próxima execução."
  } >&2
  return 1
}

# config_print_skeleton <tool_name> [<KEY>=<value>...]: prints (stdout) the config skeleton of
# the registered keys: three header lines naming <tool_name>, then, for each key in
# registration order, its comment with "# " before each line (a blank comment line becomes
# "#", with no trailing space) and KEY="<default>". Each optional <KEY>=<value> pair (NS-53:
# the init answers) writes KEY="<value>" instead of the default, literally; without pairs the
# output is the same as before. A pair whose KEY isn't registered (or without "=") is refused
# before anything is printed: a message on stderr and return 1. Checking the value is the
# caller's job (config_init refuses quotes and control characters). The file must stay
# friendly to the target team's generic text hooks (pre-commit
# trailing-whitespace/end-of-file-fixer/mixed-line-ending): no shebang, no trailing spaces,
# LF, ending with a newline — config_register_key refuses comments that would break that.
# printf '%s\n', not echo, so nothing in the text is interpreted.
config_print_skeleton() {
  local tool_name="$1" index line pair value
  shift
  for pair in "$@"; do
    case "$pair" in
      *=*) config_key_index "${pair%%=*}" > /dev/null && continue ;;
    esac
    echo "Erro: config_print_skeleton: par inválido '$pair' (use CHAVE=valor com uma chave registrada)." >&2
    return 1
  done
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
    value="${CONFIG_KEY_DEFAULTS[$index]}"
    for pair in "$@"; do
      if [ "${pair%%=*}" = "${CONFIG_KEYS[$index]}" ]; then
        value="${pair#*=}"
      fi
    done
    printf '%s\n' "${CONFIG_KEYS[$index]}=\"$value\""
  done
}

# config_dir_prepare: creates the $CONFIG_NICK_DIR_NAME tree in the cwd —
# $CONFIG_EXCHANGE_DIR_NAME, $CONFIG_DOCS_DIR_NAME and, only if it doesn't exist yet,
# $CONFIG_NICK_DIR_NAME/.gitignore with exactly "*", "!.gitignore" and "!nick.conf"
# (everything in .nick/ is ignored but the config and the .gitignore itself, so the target
# project's own .gitignore is never touched). Not nick.conf (config_init's job) nor docs/<ID>/
# (the agent creates it when it writes there). An existing .gitignore, edited or not, and
# whatever is already in troca/ and docs/ are kept; no .gitignore is created inside troca/ or
# docs/. Prints an error on stderr and returns 1 when the tree can't be created (without exit).
config_dir_prepare() {
  local ignore="$CONFIG_NICK_DIR_NAME/.gitignore"
  if mkdir -p "$CONFIG_EXCHANGE_DIR_NAME" "$CONFIG_DOCS_DIR_NAME" 2>/dev/null; then
    [ -e "$ignore" ] && return 0
    # Grouped so the 2>/dev/null also covers the failing > "$ignore" (redirections apply left
    # to right): the user sees only the message below, not bash's own error.
    { printf '%s\n' '*' '!.gitignore' "!$(basename "$CONFIG_CWD_FILE_NAME")" > "$ignore"; } 2>/dev/null && return 0
  fi
  echo "Erro: não foi possível criar a estrutura $CONFIG_NICK_DIR_NAME/ (.gitignore, troca/ e docs/) em $PWD." >&2
  return 1
}

# config_init <tool_name>: creates the project config in the cwd — the only destination,
# $PWD/$CONFIG_CWD_FILE_NAME (config_cwd_file) — with the skeleton of the registered keys
# (config_print_skeleton <tool_name>, so the header names the tool). The user opens it and
# fills it in afterwards.
#   - An existing file is only overwritten after confirmation (s/S/sim/Sim/SIM). Any other
#     answer prints "Cancelado. Nada foi alterado." on stdout and returns 0; a closed stdin
#     (Ctrl+D) prints the same on stderr and returns 1. Either way nothing is created or
#     changed.
#   - Then, still before creating anything, each key registered with a question (the 5th
#     argument of config_register_key) is asked, in registration order, and the answer goes
#     into the skeleton (config_print_skeleton <tool_name> KEY=answer...). The answer is
#     trimmed at both ends like read -r with the default IFS. Enter and EOF (Ctrl+D) write
#     "" and go on — EOF doesn't cancel here, so an init with stdin closed still writes the
#     skeleton. An answer with a double quote or a control character (the reader takes no
#     quote inside a value) is refused without echoing it, and the question is asked again.
#   - Then config_dir_prepare creates the whole .nick/ tree (the team sees and versions it
#     from the first commit), the skeleton is written and the output shows the tree.
#   - An old $PWD/$CONFIG_CWD_LEGACY_FILE_NAME doesn't stop it (the old file is the source
#     to copy the values from): the skeleton is written all the same, the old file is neither
#     read nor touched, and a warning on stderr says to copy its values into the new file and
#     remove it — without it, the values would be lost silently.
# It never reads the config (no config_load nor config_cwd_legacy_check), so it prints no
# config warning. Returns 1, without exit, with the library's own message on stderr (never
# bash's raw error), when <tool_name> is missing or empty, or when the tree or the file can't
# be written.
config_init() {
  local tool_name file confirm index answer answers=()
  # Checked before touching "$1", so a caller under set -u gets this message, not bash's.
  if [ "$#" -ne 1 ] || [ -z "$1" ]; then
    echo "Erro: config_init espera 1 argumento (o nome da ferramenta), recebeu $#." >&2
    return 1
  fi
  tool_name="$1"
  file="$(config_cwd_file)"
  if [ -f "$file" ]; then
    # Not left to set -e: the caller may not have it, and EOF must still cancel.
    if ! read -r -p "Já existe $file. Sobrescrever? (s/N): " confirm; then
      echo "Cancelado. Nada foi alterado." >&2
      return 1
    fi
    case "$confirm" in
      s | S | sim | Sim | SIM) ;;
      *)
        echo "Cancelado. Nada foi alterado."
        return 0
        ;;
    esac
  fi
  for index in "${!CONFIG_KEYS[@]}"; do
    [ -n "${CONFIG_KEY_PROMPTS[$index]}" ] || continue
    while :; do
      # The default IFS spelled out, so the trim doesn't depend on the caller's IFS. EOF
      # writes "", whatever was typed before it.
      if ! IFS=$' \t\n' read -r -p "${CONFIG_KEY_PROMPTS[$index]}" answer; then
        answer=""
        break
      fi
      case "$answer" in
        *\"* | *[[:cntrl:]]*)
          echo "Erro: a resposta não pode ter aspas duplas (\") nem caracteres de controle. Tente de novo." >&2
          ;;
        *) break ;;
      esac
    done
    answers+=("${CONFIG_KEYS[$index]}=$answer")
  done
  # Before the skeleton, since nick.conf lives inside .nick/.
  config_dir_prepare || return 1
  # Grouped so the 2>/dev/null also covers the failing > "$file" (same as config_dir_prepare).
  if ! { config_print_skeleton "$tool_name" "${answers[@]}" > "$file"; } 2>/dev/null; then
    echo "Erro: não foi possível gravar $file." >&2
    return 1
  fi
  echo "Projeto criado em $file — edite antes de usar."
  cat <<EOF
Estrutura do nick em $CONFIG_NICK_DIR_NAME/:
  .gitignore   versionado: ignora tudo em $CONFIG_NICK_DIR_NAME/ menos ele e o nick.conf
  nick.conf    versionado: configuração do projeto
  troca/       não versionada: troca de arquivos com os agentes
  docs/        não versionada: plano, review e prints de cada ticket (docs/<ID>/)
Versione $CONFIG_CWD_FILE_NAME e $CONFIG_NICK_DIR_NAME/.gitignore junto com o projeto.
EOF
  if [ -e "$PWD/$CONFIG_CWD_LEGACY_FILE_NAME" ]; then
    echo "Aviso: o arquivo antigo $PWD/$CONFIG_CWD_LEGACY_FILE_NAME não foi lido nem alterado — copie os valores dele para $file e apague o antigo." >&2
  fi
  return 0
}
