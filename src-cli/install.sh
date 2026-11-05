#!/usr/bin/env bash
# OpenWork terminal installer for Linux and macOS.
# Same flow as the GUI installer: prerequisites, OpenWork, EPFL portal
# sign-in, RCP API key, provider configuration, smoke test.
# https://github.com/EPFL-ENAC/enac-openwork-installer
set -euo pipefail
export LC_ALL=C
umask 077

INSTALL_URL="https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/install.sh"

OPENWORK_RELEASE_API="https://api.github.com/repos/different-ai/openwork/releases/latest"
# Tests override it: /Applications is read only on a build host.
OPENWORK_APP_DIR="${OPENWORK_APP_DIR:-/Applications/OpenWork.app}"
# OpenWork bundles the engine it runs as a sidecar (opencode, opencode2).
ENGINE_V1="opencode"
ENGINE_V2="opencode2"
# A fixed path, not a subshell global: the caller captures the engine path with
# $(...), so only a value computed here survives cleanup().
ENGINE_EXTRACT_DIR="${TMPDIR:-/tmp}/openwork-engine.$$"
OPENWORK_FRESH_INSTALL=0

PORTAL_BASE="https://portal.rcp.epfl.ch"
PORTAL_KEYS_PAGE="$PORTAL_BASE/aiaas/keys"
PORTAL_KEYS_API="$PORTAL_BASE/aiaas/api/v1/keys"
PORTAL_NEW_KEY_MODAL="$PORTAL_BASE/aiaas/modal_key_new"
PORTAL_SHOW_SECRET="$PORTAL_BASE/aiaas/showsecret"
PORTAL_UA="openwork-installer-cli"

INSTALL_TIMEOUT=900
SHORT_TIMEOUT=120
LOGIN_TIMEOUT_SECS=600
COOKIE_POLL_INTERVAL=1
COOKIE_POLL_MAX_ERRORS=15
SESSION_REPROBE_TICKS=5
HTTP_TIMEOUT=30
SMOKE_TIMEOUT=60

PROVIDER_ID="epfl-rcp-aiaas"
OPENWORK_PROVIDER_ID="epfl-aiaas"
OPENWORK_FREE_PROVIDER_ID="openwork-free"
OPENWORK_ENV_KEY="EPFL_AIAAS_API_KEY"
OPENWORK_MODEL_LARGE="epfl-aiaas/large"
RCP_BASE_URL="https://inference-rcp.epfl.ch/v1"
OC_CONFIG_SCHEMA="https://opencode.ai/config.json"
OC_MODEL_PROVIDER="epfl-rcp-aiaas/epfl-aiaas/small"
SMOKE_PROMPT="Reply with the single word OK."
DEFAULT_KEY_NAME="OpenWork"

ENGINE_GLOBAL_ID="__openwork_engine_global__"
MACHINE_OWNERSHIP_ID="__machine_provider_template_ownership__"

# provider_definition() from src-tauri/src/openwork.rs, serialized compact with
# keys sorted by byte order. sha256 of this exact string is pinned by the Rust
# test as the machine-ownership fingerprint below.
DEFINITION_JSON='{"models":{"epfl-aiaas/large":{"attachment":true,"modalities":{"input":["text","image"],"output":["text"]},"name":"RCP AIaaS Large"},"epfl-aiaas/small":{"attachment":true,"modalities":{"input":["text","image"],"output":["text"]},"name":"RCP AIaaS Small","variants":{"high":{"reasoningEffort":"high"},"low":{"reasoningEffort":"low"},"max":{"reasoningEffort":"max"},"none":{"reasoningEffort":"none"},"xhigh":{"reasoningEffort":"xhigh"}}}},"name":"RCP AI Inference as a Service","npm":"@ai-sdk/openai-compatible","options":{"apiKey":"{env:EPFL_AIAAS_API_KEY}","baseURL":"https://inference-rcp.epfl.ch/v1"}}'
MACHINE_OWNERSHIP_FP="a41eac2a78f1a132c6a3d3c61fa8fa9f7c33b708512a288324df4b5bda224029"
ENGINE_GLOBAL_JSON="{\"disabled_providers\":[\"$OPENWORK_FREE_PROVIDER_ID\"],\"provider\":{\"$OPENWORK_PROVIDER_ID\":$DEFINITION_JSON}}"
MODEL_ROW_JSON='{"modelID":"epfl-aiaas/large","providerID":"epfl-aiaas"}'

OS=""
ARCH=""
CONFDIR=""
LOG_FILE=""
STTY_SAVED=""
SESSION_HEADER=""
SESSION_CSRF=""
MANUAL_TOKEN=""

say() { printf '%s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
sub() { printf '  %s\n' "$*"; }
warn() { printf '!! %s\n' "$*" >&2; }
die() { printf 'xx error: %s\n' "$*" >&2; exit 1; }

run_with_timeout() {
    local secs=$1
    shift
    if command -v timeout > /dev/null 2>&1; then
        timeout "$secs" "$@"
    else
        # Stock macOS has no timeout(1): keep the child's output, the smoke
        # test and the version probe read it.
        local pid rc watcher tmp
        tmp=$(mktemp)
        "$@" > "$tmp" 2>&1 & pid=$!
        ( sleep "$secs"; kill "$pid" 2> /dev/null ) 2> /dev/null & watcher=$!
        wait "$pid" 2> /dev/null
        rc=$?
        kill "$watcher" 2> /dev/null || true
        wait "$watcher" 2> /dev/null || true
        cat "$tmp"
        rm -f "$tmp"
        return "$rc"
    fi
}

sha512_file() {
    if command -v sha512sum > /dev/null 2>&1; then
        sha512sum "$1" | awk '{print $1}'
    elif command -v shasum > /dev/null 2>&1; then
        shasum -a 512 "$1" | awk '{print $1}'
    else
        openssl dgst -sha512 -r "$1" | awk '{print $1}'
    fi
}

b64_to_hex() {
    local tmp
    tmp=$(mktemp) || return 1
    if printf '%s' "$1" | base64 -d > "$tmp" 2> /dev/null && [[ -s $tmp ]]; then :
    elif printf '%s' "$1" | base64 -D > "$tmp" 2> /dev/null && [[ -s $tmp ]]; then :
    else
        rm -f "$tmp"
        return 1
    fi
    od -An -tx1 "$tmp" | tr -d ' \n'
    rm -f "$tmp"
}

now_ms() {
    printf '%s\n' $(( $(date +%s) * 1000 ))
}

canon_path() {
    local p=$1 out=""
    if out=$(readlink -f "$p" 2> /dev/null) && [[ -n $out ]]; then
        printf '%s\n' "$out"
    elif out=$(cd "$p" 2> /dev/null && pwd -P) && [[ -n $out ]]; then
        printf '%s\n' "$out"
    else
        printf '%s\n' "$p"
    fi
}

html_unescape() {
    local s=$1
    s=${s//\&amp;/\&}
    s=${s//\&lt;/<}
    s=${s//\&gt;/>}
    s=${s//\&quot;/\"}
    s=${s//\&\#39;/"'"}
    printf '%s' "$s"
}

sqlq() {
    printf '%s' "$1" | sed -e "s/'/''/g"
}

# SQL reaches sqlite3 on stdin, never as an argv word: Linux caps one exec
# argument at 128 KiB (MAX_ARG_STRLEN) and a whole JSON body overruns it with
# "Argument list too long".
jvalid() {
    [[ $(sqlite3 :memory: <<< "SELECT json_valid('$(sqlq "$1")');") == "1" ]]
}

jtype_at() {
    sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$1")', '$(sqlq "$2")');"
}

jraw_at() {
    sqlite3 :memory: <<< "SELECT json('$(sqlq "$1")') -> '$(sqlq "$2")';"
}

# Value at path as a JSON literal: raw text for objects and arrays, quoted
# literal for scalars, so it can be re-embedded in a larger JSON document.
jlit_at() {
    sqlite3 :memory: <<< "SELECT json('$(sqlq "$1")') -> '$(sqlq "$2")';"
}

# Canonical compact JSON: object keys recursively sorted by byte order, same
# shape as serde_json::to_string over serde_json BTreeMap values.
json_canonical() {
    local t
    t=$(sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$1")');")
    case $t in
        object) __canon_object "$1" ;;
        array) __canon_array "$1" ;;
        *) sqlite3 :memory: <<< "SELECT json('$(sqlq "$1")');" ;;
    esac
}

__canon_object() {
    local json=$1 keys k v out="{" first=1
    keys=$(sqlite3 :memory: <<< "SELECT key FROM json_each('$(sqlq "$json")') ORDER BY key;") || true
    while IFS= read -r k; do
        [[ -n $k ]] || continue
        v=$(json_canonical "$(jraw_at "$json" "\$.$k")")
        if [[ $first -eq 0 ]]; then out+=","; fi
        first=0
        out+="$(sqlite3 :memory: <<< "SELECT json_quote('$(sqlq "$k")');"):$v"
    done <<< "$keys"
    printf '%s}' "$out"
}

__canon_array() {
    local json=$1 n=0 i e out="[" first=1
    n=$(sqlite3 :memory: <<< "SELECT json_array_length('$(sqlq "$json")');") || n=0
    for ((i = 0; i < n; i++)); do
        e=$(sqlite3 :memory: <<< "SELECT json('$(sqlq "$json")') -> '\$[$i]';")
        if [[ $first -eq 0 ]]; then out+=","; fi
        first=0
        out+="$(json_canonical "$e")"
    done
    printf '%s]' "$out"
}

# 2-space pretty printer matching JSON.stringify(value, null, 2) for compact
# JSON input. No trailing newline; callers add it.
json_pretty() {
    local s=$1 out="" i=0 n=${#1} c d in_str=0 esc=0 ind=""
    while ((i < n)); do
        c=${s:i:1}
        if ((in_str)); then
            out+=$c
            if ((esc)); then esc=0
            elif [[ $c == '\' ]]; then esc=1
            elif [[ $c == '"' ]]; then in_str=0
            fi
            ((i += 1))
            continue
        fi
        case $c in
            '"')
                in_str=1
                out+=$c
                ;;
            '{' | '[')
                d=${s:i+1:1}
                if [[ $d == '}' || $d == ']' ]]; then
                    out+=$c$d
                    ((i += 2))
                    continue
                fi
                ind="$ind  "
                out+=$c$'\n'"$ind"
                ;;
            '}' | ']')
                ind=${ind#??}
                out+=$'\n'"$ind"$c
                ;;
            ',')
                out+=","$'\n'"$ind"
                ;;
            ':')
                out+=": "
                ;;
            [[:space:]]) ;;
            *) out+=$c ;;
        esac
        ((i += 1))
    done
    printf '%s' "$out"
}

# Strip JSONC comments and trailing commas, string-aware, like the GUI's
# stripJsonc() before merging the global OpenCode config.
jsonc_strip() {
    local s=$1 out="" i=0 j=0 n=${#1} c d in_str=0 esc=0 nd
    while ((i < n)); do
        c=${s:i:1}
        d=${s:i+1:1}
        if ((in_str)); then
            out+=$c
            if ((esc)); then esc=0
            elif [[ $c == '\' ]]; then esc=1
            elif [[ $c == '"' ]]; then in_str=0
            fi
            ((i += 1))
            continue
        fi
        if [[ $c == '"' ]]; then
            in_str=1
            out+=$c
            ((i += 1))
            continue
        fi
        if [[ $c == '/' && $d == '/' ]]; then
            while ((i < n)) && [[ ${s:i:1} != $'\n' ]]; do ((i += 1)); done
            continue
        fi
        if [[ $c == '/' && $d == '*' ]]; then
            ((i += 2))
            while ((i < n)) && [[ ${s:i:1}${s:i+1:1} != '*/' ]]; do ((i += 1)); done
            ((i += 2))
            continue
        fi
        if [[ $c == ',' ]]; then
            j=$((i + 1))
            while ((j < n)); do
                d=${s:j:1}
                [[ $d == ' ' || $d == $'\t' || $d == $'\n' || $d == $'\r' ]] || break
                ((j += 1))
            done
            nd=${s:j:1}
            if [[ $nd == '}' || $nd == ']' ]]; then
                ((i += 1))
                continue
            fi
        fi
        out+=$c
        ((i += 1))
    done
    printf '%s' "$out"
}

oc_provider_block() {
    printf '{"name":"RCP AI Inference as a Service","package":"aisdk:@ai-sdk/openai-compatible","settings":{"baseURL":"%s","apiKey":"%s"},"models":{"epfl-aiaas/large":{"name":"RCP AIaaS Large","modelID":"epfl-aiaas/large","capabilities":{"input":["image","text"],"output":["text"]},"cost":{"input":0.1664,"output":0.4991,"cache":{"read":0.0166}},"settings":{}},"epfl-aiaas/small":{"name":"RCP AIaaS Small","modelID":"epfl-aiaas/small","capabilities":{"input":["image","text"],"output":["text"]},"cost":{"input":0.1671,"output":0.5014,"cache":{"read":0.0167}},"settings":{"reasoningEffort":"none"},"variants":[{"id":"none","settings":{"reasoningEffort":"none"}},{"id":"low","settings":{"reasoningEffort":"low"}},{"id":"high","settings":{"reasoningEffort":"high"}},{"id":"xhigh","settings":{"reasoningEffort":"xhigh"}},{"id":"max","settings":{"reasoningEffort":"max"}}]}}}' "$RCP_BASE_URL" "$1"
}

# JS insertion-order merge: $schema first, base keys in order with providers
# replaced in place (or appended last), the RCP provider replaced wholesale.
merge_oc_config() {
    local base=$1 token=$2 block prov_val schema_val k v keys has_prov=0 out
    block=$(oc_provider_block "$token")
    prov_val=$(sqlite3 :memory: <<< "SELECT json_set(COALESCE(json_extract('$(sqlq "$base")','$.providers'), json('{}')), '\$.\"$PROVIDER_ID\"', json('$(sqlq "$block")'));")
    schema_val=$(sqlite3 :memory: <<< "SELECT json_quote(COALESCE(json_extract('$(sqlq "$base")','\$.\"\$schema\"'), '$OC_CONFIG_SCHEMA'));")
    out='{"$schema":'"$schema_val"
    keys=$(sqlite3 :memory: <<< "SELECT key FROM json_each('$(sqlq "$base")');") || true
    while IFS= read -r k; do
        [[ -n $k ]] || continue
        if [[ $k == '$schema' ]]; then continue; fi
        if [[ $k == "providers" ]]; then
            has_prov=1
            out+=',"providers":'"$prov_val"
            continue
        fi
        v=$(jlit_at "$base" "\$.$k")
        out+=",$(sqlite3 :memory: <<< "SELECT json_quote('$(sqlq "$k")');"):$v"
    done <<< "$keys"
    [[ $has_prov -eq 1 ]] || out+=',"providers":'"$prov_val"
    out+='}'
    printf '%s' "$out"
}

write_opencode_config() {
    local token=$1
    local dir file bak raw stripped base merged existing_norm
    dir="$HOME/.config/opencode"
    file="$dir/opencode.json"
    bak="$dir/opencode.json.bak"
    mkdir -p "$dir"
    base='{}'
    if [[ -f $file ]]; then
        [[ -f $bak ]] || cp "$file" "$bak"
        chmod 600 "$bak" 2> /dev/null || true
        raw=$(cat "$file")
        stripped=$(jsonc_strip "$raw")
        if ! jvalid "$stripped"; then
            die "$file is not valid JSON or JSONC, fix it and rerun the installer"
        fi
        if [[ $(sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$stripped")');") != "object" ]]; then
            die "$file is not a JSON object, fix it and rerun the installer"
        fi
        base=$(sqlite3 :memory: <<< "SELECT json('$(sqlq "$stripped")');")
    fi
    merged=$(merge_oc_config "$base" "$token")
    existing_norm=$base
    if [[ $merged == "$existing_norm" ]]; then
        sub "OpenCode provider config already up to date ($file)"
        return 0
    fi
    printf '%s\n' "$(json_pretty "$merged")" > "$file"
    chmod 600 "$file"
    sub "wrote $file (backup at $bak)"
}

write_env_store() {
    local token=$1
    local file raw base cur entry out vars k vt placed tmp now
    file="$CONFDIR/env.json"
    now=$(now_ms)
    base="{\"schemaVersion\":1,\"updatedAt\":$now,\"variables\":[]}"
    if [[ -f $file ]]; then
        raw=$(cat "$file")
        if ! jvalid "$raw"; then
            die "$file is not valid JSON, fix or remove it and rerun the installer"
        fi
        if [[ $(sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$raw")');") != "object" ]]; then
            die "$file is not a JSON object, fix or remove it and rerun the installer"
        fi
        vt=$(sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$raw")','\$.variables');")
        if [[ -n $vt && $vt != "array" ]]; then
            die "$file holds no variable list, fix or remove it and rerun the installer"
        fi
        if [[ $vt == "array" ]] \
            && [[ $(sqlite3 :memory: <<< "SELECT count(*) FROM json_each('$(sqlq "$raw")','\$.variables') WHERE json_type(value) IS NULL OR json_type(value) != 'object';") != "0" ]]; then
            die "$file holds a variable that is not an object, fix or remove it and rerun the installer"
        fi
        base=$raw
        cur=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.value') FROM json_each('$(sqlq "$base")','\$.variables') WHERE json_extract(value,'\$.key')='$OPENWORK_ENV_KEY';")
        if [[ $cur == "$token" ]]; then
            sub "OpenWork env store already up to date ($file)"
            return 0
        fi
    fi
    entry="{\"key\":\"$OPENWORK_ENV_KEY\",\"updatedAt\":$now,\"value\":$(sqlite3 :memory: <<< "SELECT json_quote('$token');")}"
    vars=""
    placed=0
    while IFS=$'\t' read -r k v; do
        # A missing or null key decodes to an empty k: the entry survives
        # verbatim, like merge_env_store skipping it.
        [[ -n $k ]] || continue
        k=$(sqlite3 :memory: <<< "SELECT json_extract('$(sqlq "$k")','\$');")
        if [[ $placed -eq 0 && $k == "$OPENWORK_ENV_KEY" ]]; then continue; fi
        if [[ $placed -eq 0 && $k > "$OPENWORK_ENV_KEY" ]]; then
            vars+=",$entry"
            placed=1
        fi
        vars+=",$v"
    done < <(sqlite3 -separator $'\t' :memory: <<< "SELECT CASE WHEN json_type(value,'\$.key') == 'text' THEN json_quote(json_extract(value,'\$.key')) ELSE 'null' END, json(value) FROM json_each('$(sqlq "$base")','\$.variables') ORDER BY json_extract(value,'\$.key');") || true
    if [[ $placed -eq 0 ]]; then vars+=",$entry"; fi
    # json_set over the existing store, so foreign top-level keys and a non-1
    # schemaVersion survive, like merge_env_store mutating the object in place.
    sv=$(sqlite3 :memory: <<< "SELECT CASE WHEN json_type('$(sqlq "$base")','\$.schemaVersion') IN ('integer','real') THEN json_extract('$(sqlq "$base")','\$.schemaVersion') ELSE 1 END;")
    vars=${vars#,}
    out=$(sqlite3 :memory: <<< "SELECT json_set('$(sqlq "$base")','\$.schemaVersion',$sv,'\$.updatedAt',$now,'\$.variables', json('[$vars]'));")
    tmp="$file.tmp"
    printf '%s\n' "$(json_pretty "$(json_canonical "$out")")" > "$tmp"
    chmod 600 "$tmp"
    mv -f "$tmp" "$file"
    sub "wrote $file"
}

upsert_row() {
    local db=$1 table=$2 col=$3 ws_id=$4 value=$5 now=$6
    sqlite3 -cmd '.timeout 5000' "$db" <<< "INSERT INTO $table (workspace_id, $col, updated_at) VALUES ('$ws_id', '$(sqlq "$value")', $now) ON CONFLICT(workspace_id) DO UPDATE SET $col = excluded.$col, updated_at = excluded.updated_at;"
}

read_config_row() {
    local db=$1 table=$2 ws_id=$3
    sqlite3 -cmd '.timeout 5000' "$db" <<< "SELECT $4 FROM $table WHERE workspace_id = '$ws_id';" 2> /dev/null || true
}

write_runtime_rows() {
    local db="$CONFDIR/runtime.sqlite" now
    now=$(now_ms)
    sqlite3 -cmd '.timeout 5000' "$db" <<'SQL'
CREATE TABLE IF NOT EXISTS runtime_opencode_configs (workspace_id TEXT PRIMARY KEY NOT NULL, config_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS openwork_workspace_configs (workspace_id TEXT PRIMARY KEY NOT NULL, config_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS workspace_default_models (workspace_id TEXT PRIMARY KEY NOT NULL, model_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
SQL
    chmod 600 "$db"

    local existing new dt pm ptype
    existing=$(read_config_row "$db" runtime_opencode_configs "$ENGINE_GLOBAL_ID" config_json)
    if [[ -z $existing ]]; then
        new=$ENGINE_GLOBAL_JSON
    else
        if ! jvalid "$existing"; then
            die "runtime.sqlite holds a corrupt row for $ENGINE_GLOBAL_ID, remove it and rerun"
        fi
        if [[ $(sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$existing")');") != "object" ]]; then
            die "the OpenWork runtime config row for $ENGINE_GLOBAL_ID is not a JSON object, remove it and rerun"
        fi
        ptype=$(jtype_at "$existing" "\$.provider")
        if [[ $ptype == "object" ]]; then
            pm=$(sqlite3 :memory: <<< "SELECT json_set(json_extract('$(sqlq "$existing")','\$.provider'), '\$.\"$OPENWORK_PROVIDER_ID\"', json('$(sqlq "$DEFINITION_JSON")'));")
        elif [[ -n $ptype ]]; then
            die "the OpenWork runtime config holds no provider map ($ENGINE_GLOBAL_ID has a non-object provider), remove the row and rerun"
        else
            pm="{\"$OPENWORK_PROVIDER_ID\":$DEFINITION_JSON}"
        fi
        new=$(sqlite3 :memory: <<< "SELECT json_set('$(sqlq "$existing")','\$.provider', json('$(sqlq "$pm")'));")
        if [[ $(jtype_at "$new" "\$.disabled_providers") != "array" ]]; then
            new=$(sqlite3 :memory: <<< "SELECT json_set('$(sqlq "$new")','\$.disabled_providers', json('[\"$OPENWORK_FREE_PROVIDER_ID\"]'));")
        elif ! sqlite3 :memory: <<< "SELECT 1 FROM json_each('$(sqlq "$new")','\$.disabled_providers') WHERE value='$OPENWORK_FREE_PROVIDER_ID';" | grep -q 1; then
            dt=$(sqlite3 :memory: <<< "SELECT json_group_array(value) FROM (SELECT value FROM json_each('$(sqlq "$new")','\$.disabled_providers') UNION ALL SELECT '$OPENWORK_FREE_PROVIDER_ID');")
            new=$(sqlite3 :memory: <<< "SELECT json_set('$(sqlq "$new")','\$.disabled_providers', json('$(sqlq "$dt")'));")
        fi
        new=$(json_canonical "$new")
    fi
    if [[ $new != "$existing" ]]; then
        upsert_row "$db" runtime_opencode_configs config_json "$ENGINE_GLOBAL_ID" "$new" "$now"
        sub "wrote the engine global provider row ($ENGINE_GLOBAL_ID)"
    else
        sub "engine global provider row already up to date"
    fi

    local existing_own cur base_own new_own
    existing_own=$(read_config_row "$db" openwork_workspace_configs "$MACHINE_OWNERSHIP_ID" config_json)
    cur=""
    base_own='{}'
    if [[ -n $existing_own ]]; then
        if ! jvalid "$existing_own"; then
            die "the machine ownership row $MACHINE_OWNERSHIP_ID is not valid JSON, remove it and rerun"
        fi
        if [[ $(sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$existing_own")');") != "object" ]]; then
            die "the machine ownership row $MACHINE_OWNERSHIP_ID is not a JSON object, remove it and rerun"
        fi
        base_own=$existing_own
        cur=$(sqlite3 :memory: <<< "SELECT json_extract('$(sqlq "$base_own")','\$.\"$OPENWORK_PROVIDER_ID\"');")
    fi
    if [[ $cur != "$MACHINE_OWNERSHIP_FP" ]]; then
        new_own=$(sqlite3 :memory: <<< "SELECT json_set('$(sqlq "$base_own")','\$.\"$OPENWORK_PROVIDER_ID\"','$MACHINE_OWNERSHIP_FP');")
        new_own=$(json_canonical "$new_own")
        upsert_row "$db" openwork_workspace_configs config_json "$MACHINE_OWNERSHIP_ID" "$new_own" "$now"
        sub "wrote the machine provider template ownership row"
    else
        sub "machine provider template ownership row already up to date"
    fi
}

# Remove provider.epfl-aiaas from a workspace opencode.jsonc, with the same
# semantics as the GUI heal: comments or invalid JSON stay untouched, a file
# reduced to only $schema is deleted, rewrites are pretty-printed and sorted.
heal_workspace_config() {
    local file content next keys
    file="$1/opencode.jsonc"
    [[ -f $file ]] || return 0
    content=$(cat "$file")
    if ! jvalid "$content"; then
        sub "left $file untouched (comments or invalid JSON)"
        return 0
    fi
    if [[ $(sqlite3 :memory: <<< "SELECT json_type('$(sqlq "$content")');") != "object" ]]; then
        sub "left $file untouched (not a JSON object)"
        return 0
    fi
    if [[ $(jtype_at "$content" "\$.provider") != "object" ]] \
        || [[ $(jtype_at "$content" "\$.provider.\"$OPENWORK_PROVIDER_ID\"") == "null" ]]; then
        return 0
    fi
    next=$(sqlite3 :memory: <<< "SELECT json_remove('$(sqlq "$content")', '\$.provider.\"$OPENWORK_PROVIDER_ID\"');")
    if [[ $(sqlite3 :memory: <<< "SELECT count(*) FROM json_each('$(sqlq "$next")','\$.provider');" 2> /dev/null || echo 1) == "0" ]]; then
        next=$(sqlite3 :memory: <<< "SELECT json_remove('$(sqlq "$next")', '\$.provider');")
    fi
    keys=$(sqlite3 :memory: <<< "SELECT count(*) FROM json_each('$(sqlq "$next")') WHERE key != '\$schema';")
    if [[ $keys == "0" ]]; then
        rm -f "$file"
        sub "removed $file (only a schema key remained)"
        return 0
    fi
    printf '%s\n' "$(json_pretty "$(json_canonical "$next")")" > "$file"
    sub "healed $file (removed the stale $OPENWORK_PROVIDER_ID provider)"
}

resolve_workspace() {
    local state_file state v entry ws_id="" ws_path=""
    if [[ $OS == "macos" ]]; then
        state_file="$HOME/Library/Application Support/com.differentai.openwork/openwork-workspaces.json"
    else
        state_file="${XDG_CONFIG_HOME:-$HOME/.config}/com.differentai.openwork/openwork-workspaces.json"
    fi
    if [[ ! -e $state_file ]]; then
        ws_path="$HOME/OpenWork Chat"
        mkdir -p "$ws_path" 2> /dev/null || true
        ws_path=$(canon_path "$ws_path")
        ws_id="ws_$(printf '%s' "$ws_path" | sha256sum | cut -c1-12)"
        heal_workspace_config "$ws_path"
        printf '%s\n' "$ws_id"
        return 0
    fi
    state=$(cat "$state_file")
    if ! jvalid "$state"; then
        warn "cannot read $state_file, skipping workspace configuration"
        return 1
    fi
    local key
    for key in selectedId selectedWorkspaceId activeId activeWorkspaceId watchedId watchedWorkspaceId; do
        v=$(sqlite3 :memory: <<< "SELECT json_extract('$(sqlq "$state")','\$.\"$key\"');" 2> /dev/null || true)
        if [[ -n $v && $v != "null" ]]; then
            ws_id=$v
            break
        fi
    done
    entry=""
    if [[ -n $ws_id ]]; then
        entry=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.path')||char(9)||COALESCE(json_extract(value,'\$.workspaceType'),'local') FROM json_each('$(sqlq "$state")','\$.workspaces') WHERE json_extract(value,'\$.id')='$ws_id' LIMIT 1;") || true
    fi
    if [[ -z $ws_id ]]; then
        # No usable selection key: fall back to the first local workspace, like
        # pick_workspace_id. A selected id missing from workspaces[] keeps its
        # id (path None in resolve_default_workspace), it only skips the heal.
        entry=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.id')||char(9)||json_extract(value,'\$.path')||char(9)||COALESCE(json_extract(value,'\$.workspaceType'),'local') FROM json_each('$(sqlq "$state")','\$.workspaces') WHERE COALESCE(json_extract(value,'\$.workspaceType'),'local')='local' LIMIT 1;") || true
        if [[ -n $entry ]]; then
            ws_id=$(printf '%s' "$entry" | cut -f1)
            entry=$(printf '%s' "$entry" | cut -f2-)
        fi
    fi
    if [[ -z $ws_id ]]; then
        warn "found no workspace in $state_file, skipping workspace configuration"
        return 1
    fi
    if [[ -n $entry ]]; then
        ws_path=$(printf '%s' "$entry" | cut -f1)
        local wtype
        wtype=$(printf '%s' "$entry" | cut -f2)
        if [[ $wtype == "local" && -n $ws_path ]]; then
            heal_workspace_config "$ws_path"
        fi
    fi
    printf '%s\n' "$ws_id"
}

configure_workspace() {
    local ws_id
    if ws_id=$(resolve_workspace); then
        local db="$CONFDIR/runtime.sqlite" now existing_dm
        now=$(now_ms)
        existing_dm=$(read_config_row "$db" workspace_default_models "$ws_id" model_json)
        if [[ $existing_dm != "$MODEL_ROW_JSON" ]]; then
            upsert_row "$db" workspace_default_models model_json "$ws_id" "$MODEL_ROW_JSON" "$now"
            sub "set the default model for workspace $ws_id to $OPENWORK_MODEL_LARGE (RCP AIaaS Large)"
        else
            sub "default model for workspace $ws_id already up to date"
        fi
    fi
}

disable_telemetry() {
    local file="$CONFDIR/free-auto-analytics" current=""
    [[ -f $file ]] && current=$(tr -d '[:space:]' < "$file" || true)
    if [[ $current != "disabled" ]]; then
        printf 'disabled' > "$file"
        chmod 600 "$file"
        sub "disabled OpenWork telemetry ($file)"
    else
        sub "OpenWork telemetry already disabled"
    fi
    rm -f "$CONFDIR/free-auto-journey"
}

check_prereqs() {
    local c
    for c in curl sqlite3; do
        command -v "$c" > /dev/null 2>&1 || die "$c is required but missing (on Ubuntu: sudo apt install $c)"
    done
    if [[ $OS == "macos" ]]; then
        command -v open > /dev/null 2>&1 || die "the open command is required to launch the EPFL portal in a browser"
    else
        command -v xdg-open > /dev/null 2>&1 || die "xdg-open is required to launch the EPFL portal in a browser (on Ubuntu: sudo apt install xdg-utils)"
    fi
    [[ $(sqlite3 :memory: <<< "SELECT json_valid('{}');") == "1" ]] \
        || die "the sqlite3 binary lacks JSON support (json1), install a full sqlite3 build"
    [[ $(sqlite3 :memory: <<< "SELECT json('{}') -> '\$';") == "{}" ]] \
        || die "the sqlite3 binary lacks the JSON arrow operator (3.38+), install a newer sqlite3"
    # The GUI stays silent when ldconfig is unavailable; webkit sonames are GUI-only.
    if [[ $OS == "linux" ]] && command -v ldconfig > /dev/null 2>&1 && ! ldconfig -p 2> /dev/null | grep -q "libfuse.so.2"; then
        warn "libfuse2 is missing, the OpenWork AppImage may need to run with --appimage-extract-and-run"
    fi
    sub "curl, sqlite3 and the browser opener are present"
    sub "architecture: $OS/$ARCH"
}

check_network() {
    if curl -sS -m 5 -o /dev/null "$PORTAL_BASE" 2> /dev/null; then
        sub "network: $PORTAL_BASE is reachable"
    else
        die "no network: $PORTAL_BASE is not reachable (connect to the EPFL network or VPN and rerun)"
    fi
}

engine_triple() {
    case "$OS:$ARCH" in
        "macos:aarch64") printf 'aarch64-apple-darwin' ;;
        "macos:x86_64") printf 'x86_64-apple-darwin' ;;
        "linux:x86_64") printf 'x86_64-unknown-linux-gnu' ;;
        "linux:aarch64") printf 'aarch64-unknown-linux-gnu' ;;
        *) return 1 ;;
    esac
}

# Bundled sidecars ship a plain name and a target-triple suffixed copy.
engine_binary_in() {
    local dir=$1 name=$2 triple c
    [[ -d $dir ]] || return 1
    triple=$(engine_triple) || return 1
    for c in "$dir/$name" "$dir/$name-$triple"; do
        if [[ -f $c ]]; then
            printf '%s\n' "$c"
            return 0
        fi
    done
    return 1
}

# Mirrors resolveFreshInstallEngine: this script's own fresh install runs v2, an
# already present install keeps v1 unless the preview marker or env asks for v2.
openwork_engine_name() {
    local preview=${OPENWORK_ENGINE_V2_PREVIEW:-}
    preview=${preview//[[:space:]]/}
    if [[ $OPENWORK_FRESH_INSTALL == "1" || -n $preview || -f "$CONFDIR/engine-v2-preview.json" ]]; then
        printf '%s\n' "$ENGINE_V2"
        return 0
    fi
    printf '%s\n' "$ENGINE_V1"
}

openwork_appimage() {
    local dest="$HOME/.local/share/openwork/OpenWork.AppImage" exec_line
    if [[ -x $dest ]]; then
        printf '%s\n' "$dest"
        return 0
    fi
    exec_line=$(sed -n 's/^Exec="\([^"]*\)".*/\1/p' "$HOME/.local/share/applications/openwork.desktop" 2> /dev/null | head -1 || true)
    if [[ $exec_line == *.AppImage && -x $exec_line ]]; then
        printf '%s\n' "$exec_line"
        return 0
    fi
    return 1
}

locate_openwork_engine() {
    local name installed appimage
    name=$(openwork_engine_name)
    if [[ $OS == "macos" ]]; then
        engine_binary_in "$OPENWORK_APP_DIR/Contents/Resources/sidecars" "$name"
        return $?
    fi
    installed="$HOME/.local/share/openwork/resources/sidecars"
    if engine_binary_in "$installed" "$name"; then
        return 0
    fi
    appimage=$(openwork_appimage) || return 1
    extract_appimage_engine "$appimage" "$name"
}

# FUSE-free read of the bundled sidecars out of the AppImage. The temp dir stays
# until cleanup() so the caller can run the engine it points at.
extract_appimage_engine() {
    local appimage=$1 name=$2 sidecars
    sidecars="$ENGINE_EXTRACT_DIR/squashfs-root/resources/sidecars"
    rm -rf "$ENGINE_EXTRACT_DIR"
    mkdir -p "$ENGINE_EXTRACT_DIR" || return 1
    if ! (cd "$ENGINE_EXTRACT_DIR" && run_with_timeout "$INSTALL_TIMEOUT" "$appimage" --appimage-extract '*sidecars*' > /dev/null 2>&1) \
        || ! engine_binary_in "$sidecars" "$name"; then
        rm -rf "$ENGINE_EXTRACT_DIR"
        return 1
    fi
    engine_binary_in "$sidecars" "$name"
}

repair_opencode_state() {
    local d f stamp probe
    local -a dirs=()
    # Same scan set as database_state_dirs: XDG when set, the usual dir, and
    # the macOS legacy path on every OS (it simply does not exist on Linux).
    [[ -n ${XDG_DATA_HOME:-} ]] && dirs+=("${XDG_DATA_HOME}/opencode")
    dirs+=("$HOME/.local/share/opencode" "$HOME/Library/Application Support/opencode")
    stamp=$(date +%s)
    for d in "${dirs[@]}"; do
        [[ -d $d ]] || continue
        for f in "$d"/*.db; do
            [[ -f $f ]] || continue
            # Foreign like is_foreign: at least one table, none named session.
            probe=$(sqlite3 "$f" <<< "SELECT CASE WHEN (SELECT count(*) FROM sqlite_master WHERE type='table') > 0 AND NOT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='session') THEN 1 ELSE 0 END;" 2> /dev/null || true)
            if [[ $probe == "1" ]]; then
                mv "$f" "$f.bak-$stamp"
                for s in -wal -shm; do
                    [[ -f $f$s ]] && mv "$f$s" "$f$s.bak-$stamp"
                done
                sub "backed up the foreign state database $f"
            fi
        done
    done
}

github_release_json() {
    curl -fsSL --max-time "$HTTP_TIMEOUT" -H "Accept: application/vnd.github+json" -H "User-Agent: $PORTAL_UA" "$OPENWORK_RELEASE_API" \
        || die "could not read the OpenWork release list from GitHub"
}

verify_sha512() {
    local file=$1 asset=$2 meta_url=$3 meta expected actual
    meta=$(mktemp)
    if ! curl -fsSL --max-time "$HTTP_TIMEOUT" -H "User-Agent: $PORTAL_UA" -o "$meta" "$meta_url"; then
        rm -f "$meta"
        warn "the release holds no $asset metadata, checksum skipped"
        return 0
    fi
    expected=$(awk '
        /^[[:space:]]*-[[:space:]]*url:/ { u=$0; sub(/^[[:space:]]*-[[:space:]]*url:[[:space:]]*/, "", u); name=u }
        /^[[:space:]]*sha512:/ { h=$0; sub(/^[[:space:]]*sha512:[[:space:]]*/, "", h); print name " " h }
    ' "$meta" | awk -v n="$asset" '$1 == n {print $2}')
    rm -f "$meta"
    if [[ -z $expected ]]; then
        warn "the metadata lists no sha512 for $asset, checksum skipped"
        return 0
    fi
    if ! expected=$(b64_to_hex "$expected"); then
        die "the published sha512 for $asset is not valid base64"
    fi
    actual=$(sha512_file "$file")
    if [[ $actual != "$expected" ]]; then
        die "the downloaded file $asset does not match the published sha512"
    fi
    sub "checksum verified for $asset" >&2
}

download_release_asset() {
    local release=$1 prefix=$2 suffix=$3 dest=$4
    local version asset url meta_name
    version=$(sqlite3 :memory: <<< "SELECT json_extract('$(sqlq "$release")','\$.tag_name');")
    version=${version#v}
    asset="${prefix}${version}${suffix}"
    case $asset in
        *mac-*) meta_name="latest-mac.yml" ;;
        *arm64.AppImage) meta_name="latest-linux-arm64.yml" ;;
        *) meta_name="latest-linux.yml" ;;
    esac
    url=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.browser_download_url') FROM json_each('$(sqlq "$release")','\$.assets') WHERE json_extract(value,'\$.name')='$asset' LIMIT 1;")
    [[ -n $url && $url != "null" ]] || die "the OpenWork release holds no asset named $asset"
    meta_url=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.browser_download_url') FROM json_each('$(sqlq "$release")','\$.assets') WHERE json_extract(value,'\$.name')='$meta_name' LIMIT 1;")
    say "downloading $asset" >&2
    curl -fL --max-time "$INSTALL_TIMEOUT" -H "User-Agent: $PORTAL_UA" -o "$dest" "$url" \
        || die "could not download $url"
    verify_sha512 "$dest" "$asset" "$meta_url"
    printf '%s\n' "$version"
}

write_desktop_entry() {
    local appimage=$1 dir icon
    dir="$HOME/.local/share"
    icon="application-x-executable"
    [[ -f "$dir/openwork.png" ]] && icon="$dir/openwork.png"
    mkdir -p "$dir/applications"
    cat > "$dir/applications/openwork.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=OpenWork
Exec="$appimage"
Icon=$icon
Terminal=false
Categories=Development;
EOF
    sub "wrote $dir/applications/openwork.desktop"
}

install_openwork_linux() {
    local dest_dir="$HOME/.local/share/openwork" dest="$HOME/.local/share/openwork/OpenWork.AppImage" existing
    if existing=$(openwork_appimage); then
        sub "OpenWork is already installed at $existing, skipping"
        return 0
    fi
    local tmp release version
    tmp=$(mktemp -d)
    release=$(github_release_json)
    local prefix="openwork-linux-arm64-"
    [[ $ARCH == "x86_64" ]] && prefix="openwork-linux-x86_64-"
    version=$(download_release_asset "$release" "$prefix" ".AppImage" "$tmp/openwork.AppImage")
    mkdir -p "$dest_dir"
    rm -f "$dest"
    cp "$tmp/openwork.AppImage" "$dest"
    chmod 755 "$dest"
    printf '%s\n' "$version" > "$dest_dir/OpenWork.version"
    local icon_tmp biggest f sz icon=""
    icon_tmp=$(mktemp -d)
    (cd "$icon_tmp" && run_with_timeout "$SHORT_TIMEOUT" "$dest" --appimage-extract '*.png' > /dev/null 2>&1) || true
    if [[ -d $icon_tmp/squashfs-root ]]; then
        biggest=0
        while IFS= read -r f; do
            sz=$(wc -c < "$f")
            if ((sz > biggest)); then
                biggest=$sz
                icon=$f
            fi
        done < <(find "$icon_tmp/squashfs-root" -type f -name '*.png')
        [[ -n $icon ]] && cp "$icon" "$dest_dir/openwork.png"
    fi
    rm -rf "$icon_tmp" "$tmp"
    write_desktop_entry "$dest" 
    sub "installed OpenWork $version at $dest"
}

install_openwork_macos() {
    local dest="$OPENWORK_APP_DIR"
    if [[ -d $dest ]]; then
        sub "OpenWork is already installed at $dest, skipping"
        return 0
    fi
    local tmp release mp
    tmp=$(mktemp -d)
    mp="$tmp/mount"
    mkdir -p "$mp"
    release=$(github_release_json)
    local prefix="openwork-mac-arm64-"
    [[ $ARCH == "x86_64" ]] && prefix="openwork-mac-x64-"
    download_release_asset "$release" "$prefix" ".dmg" "$tmp/openwork.dmg" > /dev/null
    run_with_timeout "$SHORT_TIMEOUT" hdiutil attach "$tmp/openwork.dmg" -nobrowse -noverify -noautoopen -quiet -mountpoint "$mp" \
        || { rm -rf "$tmp"; die "could not mount the OpenWork disk image"; }
    if [[ ! -d "$mp/OpenWork.app" ]]; then
        hdiutil detach "$mp" -quiet 2> /dev/null || true
        rm -rf "$tmp"
        die "the OpenWork disk image holds no OpenWork.app"
    fi
    ditto "$mp/OpenWork.app" "$dest"
    run_with_timeout "$SHORT_TIMEOUT" hdiutil detach "$mp" -quiet 2> /dev/null || true
    rm -rf "$tmp"
    sub "installed OpenWork at $dest"
}

install_openwork() {
    step "[2/5] Installing OpenWork"
    repair_opencode_state
    # A fresh install runs the v2 sidecar, a present one keeps v1.
    if [[ $OS == "macos" ]]; then
        [[ -d $OPENWORK_APP_DIR ]] && OPENWORK_FRESH_INSTALL=0 || OPENWORK_FRESH_INSTALL=1
        install_openwork_macos
    else
        openwork_appimage > /dev/null && OPENWORK_FRESH_INSTALL=0 || OPENWORK_FRESH_INSTALL=1
        install_openwork_linux
    fi
}

probe_session() {
    local header=$1 code
    code=$(curl -sS --max-time "$HTTP_TIMEOUT" -o /dev/null -w '%{http_code}' \
        -H "Cookie: $header" -H "User-Agent: $PORTAL_UA" "$PORTAL_KEYS_PAGE" 2> /dev/null) || return 1
    [[ $code == 2* ]]
}

save_session() {
    printf '%s' "$SESSION_HEADER" > "$CONFDIR/portal-session"
    chmod 600 "$CONFDIR/portal-session"
}

try_saved_session() {
    local file="$CONFDIR/portal-session" header
    [[ -f $file ]] || return 1
    header=$(cat "$file")
    if [[ -n $header ]] && probe_session "$header"; then
        SESSION_HEADER=$header
        SESSION_CSRF=$(printf '%s' "$header" | sed -n 's/.*csrftoken=\([^;]*\).*/\1/p')
        sub "reusing the portal session saved by the previous run"
        return 0
    fi
    rm -f "$file"
    return 1
}

extract_pasted_session() {
    local line=$1 sid
    sid=$(printf '%s' "$line" | sed -n 's/.*sessionid[[:space:]]*=[[:space:]]*\([A-Za-z0-9_-][A-Za-z0-9_-]*\).*/\1/p')
    if [[ -z $sid && $line =~ ^[A-Za-z0-9_-]{20,}$ ]]; then
        sid=$line
    fi
    [[ -n $sid ]] || return 1
    printf '%s\n' "$sid"
}

portal_login() {
    step "[3/5] Signing in to the EPFL portal"
    if try_saved_session; then
        return 0
    fi
    local line sid code errs pending pending_ticks deadline
    say "opening $PORTAL_KEYS_PAGE in your browser"
    if [[ $OS == "macos" ]]; then
        open "$PORTAL_KEYS_PAGE" 2> /dev/null || warn "could not open a browser, open $PORTAL_KEYS_PAGE manually"
    else
        xdg-open "$PORTAL_KEYS_PAGE" 2> /dev/null || warn "could not open a browser, open $PORTAL_KEYS_PAGE manually"
    fi
    say ""
    say "Sign in with your EPFL account in the browser tab."
    say "This script cannot read the browser cookies, so while it polls, do one of:"
    say "  1. paste the sessionid cookie from the browser devtools (Application > Cookies), or"
    say "  2. paste a key secret shown on the portal keys page (it starts with sk-)."
    say "Polling every ${COOKIE_POLL_INTERVAL}s for up to ${LOGIN_TIMEOUT_SECS}s."
    say ""
    STTY_SAVED=$(stty -g 2> /dev/null || true)
    [[ -n $STTY_SAVED ]] && stty -echo 2> /dev/null || true
    pending=""
    pending_ticks=0
    errs=0
    deadline=$((SECONDS + LOGIN_TIMEOUT_SECS))
    while ((SECONDS < deadline)); do
        line=""
        if IFS= read -r -t "$COOKIE_POLL_INTERVAL" line; then
            line=${line#"${line%%[![:space:]]*}"}
            if [[ $line == *sk-* ]]; then
                MANUAL_TOKEN=$(printf '%s' "$line" | grep -oE 'sk-[A-Za-z0-9_-]+' | head -1)
                if [[ -n $MANUAL_TOKEN ]]; then
                    say "stored a pasted key secret (value masked)"
                    __restore_tty
                    return 0
                fi
            else
                sid=$(extract_pasted_session "$line" || true)
                if [[ -n $sid ]]; then
                    local paste_csrf=""
                    paste_csrf=$(printf '%s' "$line" | sed -n 's/.*csrftoken[[:space:]]*=[[:space:]]*\([A-Za-z0-9_-]*\).*/\1/p')
                    local paste_header="sessionid=$sid"
                    [[ -n $paste_csrf ]] && paste_header+="; csrftoken=$paste_csrf"
                    if probe_session "$paste_header"; then
                        SESSION_HEADER=$paste_header
                        SESSION_CSRF=$paste_csrf
                        save_session
                        say "portal session verified (sessionid masked)"
                        __restore_tty
                        return 0
                    fi
                    pending="sessionid=$sid"
                    pending_ticks=0
                    say "that sessionid is not signed in yet, keep going in the browser, it will be retried"
                fi
            fi
        fi
        if [[ -n $pending ]]; then
            pending_ticks=$((pending_ticks + 1))
            if ((pending_ticks >= SESSION_REPROBE_TICKS)); then
                if probe_session "$pending"; then
                    SESSION_HEADER=$pending
                    save_session
                    say "portal session verified (sessionid masked)"
                    __restore_tty
                    return 0
                fi
                pending_ticks=0
            fi
        fi
        # The CLI cannot read the browser cookie store, so this GET is only a
        # reachability probe; sign-in happens through the pasted values above.
        code=$(curl -sS --max-time "$HTTP_TIMEOUT" -o /dev/null -w '%{http_code}' \
            -H "User-Agent: $PORTAL_UA" "$PORTAL_KEYS_PAGE" 2> /dev/null) || code=""
        if [[ -z $code ]]; then
            errs=$((errs + 1))
            if ((errs >= COOKIE_POLL_MAX_ERRORS)); then
                __restore_tty
                die "the portal is unreachable after $COOKIE_POLL_MAX_ERRORS failed checks"
            fi
        else
            errs=0
        fi
    done
    __restore_tty
    if ask_yn "The portal sign-in timed out. Try again?"; then
        portal_login
        return 0
    fi
    die "the portal sign-in timed out"
}

__restore_tty() {
    [[ -n $STTY_SAVED ]] && stty "$STTY_SAVED" 2> /dev/null || true
    STTY_SAVED=""
}

ask_yn() {
    local reply=""
    printf '%s [Y/n]:\n' "$1" >&2
    read -r reply || return 1
    [[ -z $reply || $reply == [Yy]* ]]
}

fetch_keys_json() {
    local hdr_out="$1" resp code url
    resp=$(curl -sSL --max-time "$HTTP_TIMEOUT" \
        -H "Cookie: $SESSION_HEADER" -H "Accept: application/json" -H "User-Agent: $PORTAL_UA" \
        -o "$CONFDIR/.keys.json" -D "$hdr_out" -w '%{http_code} %{url_effective}' "$PORTAL_KEYS_API") \
        || die "could not list the RCP keys, is the portal session still valid?"
    code=${resp%% *}
    url=${resp#* }
    case $code in
        401 | 403) die "the portal session is not valid, rerun the installer to sign in again" ;;
        5*) die "the portal had a server error listing the keys, retry later" ;;
    esac
    case $url in
        "$PORTAL_BASE"/*) : ;;
        *) die "the portal redirected away from the keys page, sign in again and rerun the installer" ;;
    esac
    sqlite3 :memory: <<< "SELECT json(readfile('$CONFDIR/.keys.json'));" 2> /dev/null \
        || sqlite3 :memory: <<< "SELECT json('$(sqlq "$(cat "$CONFDIR/.keys.json")")');"
}

keys_selectable_tsv() {
    sqlite3 :memory: <<< "SELECT json_extract(value,'\$.id') || char(9) || COALESCE(json_extract(value,'\$.name'),'') || char(9) || COALESCE(json_extract(value,'\$.pool'),'') FROM json_each('$(sqlq "$1")') WHERE COALESCE(json_extract(value,'\$.is_application'),0) = 0 AND COALESCE(json_extract(value,'\$.is_legacy'),0) = 0 AND COALESCE(json_extract(value,'\$.is_enabled'),0) = 1 AND COALESCE(json_extract(value,'\$.is_pending'),0) = 0 ORDER BY json_extract(value,'\$.name');"
}

reveal_secret() {
    local key_id=$1 page token
    local hdr=(-H "Cookie: $SESSION_HEADER" -H "Referer: $PORTAL_KEYS_PAGE" -H "HX-Request: true")
    if [[ -n $SESSION_CSRF ]]; then hdr+=(-H "X-CSRFToken: $SESSION_CSRF"); fi
    page=$(curl -sSL --max-time "$HTTP_TIMEOUT" "${hdr[@]}" \
        --data-urlencode "key=$key_id" "$PORTAL_SHOW_SECRET") || die "could not read the key secret from the portal"
    token=$(printf '%s' "$page" | grep -oE '(^|[^A-Za-z0-9_-])(sk-[A-Za-z0-9_-]+)' | awk '{ v=$0; if (substr(v,1,1) != "s") v=substr(v,2); if (length(v) > 8 && length(v) > m) { m = length(v); t = v } } END { print t }')
    [[ -n $token ]] || die "the portal response held no key secret"
    API_TOKEN=$token
}

suggest_key_name() {
    local keys_json=$1 i candidate
    for ((i = 2; i < 100; i++)); do
        candidate="$DEFAULT_KEY_NAME-$i"
        if ! sqlite3 :memory: <<< "SELECT 1 FROM json_each('$(sqlq "$keys_json")') WHERE json_extract(value,'\$.name')='$candidate';" | grep -q 1; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    printf '%s\n' "$DEFAULT_KEY_NAME"
}

parse_modal_form() {
    local html=$1 tag v u
    # Form target like form_action: form tags only, hx-post then action, empty
    # and #-targets skipped. The live modal posts through an htmx footer
    # button, so no form tag matches and create_key_flow falls back to the
    # modal's final URL.
    FORM_URL=""
    while IFS= read -r tag; do
        [[ -n $tag ]] || continue
        for v in hx-post action; do
            u=$(printf '%s' "$tag" | grep -oE "$v=\"[^\"]*\"" | head -1) || true
            [[ -n $u ]] || continue
            u=${u#"$v=\""}
            u=${u%\"}
            if [[ -n $u && $u != "#"* ]]; then FORM_URL=$u; break 2; fi
        done
    done <<< "$(printf '%s' "$html" | tr '\n' ' ' | grep -oE '<form[^>]*>' || true)"
    [[ -n $FORM_URL ]] && FORM_URL=$(html_unescape "$FORM_URL")
    # CSRF like csrf_from_document: hidden input value, then the two meta
    # variants in the same order.
    MODAL_CSRF=$(printf '%s' "$html" | tr '\n' ' ' | sed -n 's/.*name="csrfmiddlewaretoken"[^>]*value="\([^"]*\)".*/\1/p' | head -1)
    if [[ -z $MODAL_CSRF ]]; then
        MODAL_CSRF=$(printf '%s' "$html" | tr '\n' ' ' | sed -n 's/.*name="csrf-token"[^>]*content="\([^"]*\)".*/\1/p' | head -1)
    fi
    if [[ -z $MODAL_CSRF ]]; then
        MODAL_CSRF=$(printf '%s' "$html" | tr '\n' ' ' | sed -n 's/.*name="csrfmiddlewaretoken"[^>]*content="\([^"]*\)".*/\1/p' | head -1)
    fi
    # Only the unit select: a greedy capture would leak the pool and usage
    # options of the sibling selects into the picker.
    local sel
    sel=$(printf '%s' "$html" | tr '\n' ' ' | sed 's/.*<select[^>]*name="unit"[^>]*>//; s#</select>.*##')
    UNITS_TSV=$(printf '%s' "$sel" \
        | grep -oE '<option[^>]*>[^<]*</option>' \
        | sed -E 's/<option[^>]*value="([^"]*)"[^>]*>([^<]*)<.*/\1\t\2/' \
        | awk -F'\t' '$2 != "" && $2 != "---------"' ) || true
}

create_key_flow() {
    local keys_json=$1
    local modal hdr_out form_url csrf name unit uchoice label resp body code final_url
    say "fetching the key creation form from the portal"
    hdr_out=$(mktemp)
    modal=$(curl -sSL --max-time "$HTTP_TIMEOUT" \
        -H "Cookie: $SESSION_HEADER" -H "Referer: $PORTAL_KEYS_PAGE" -H "User-Agent: $PORTAL_UA" \
        -o "$CONFDIR/.modal.html" -D "$hdr_out" -w '%{url_effective}' "$PORTAL_NEW_KEY_MODAL") \
        || die "could not fetch the key creation form"
    fresh_csrf_from_headers "$hdr_out"
    parse_modal_form "$(cat "$CONFDIR/.modal.html")"
    rm -f "$hdr_out" "$CONFDIR/.modal.html"
    form_url=$FORM_URL
    [[ -n $form_url ]] || form_url=$modal
    case $form_url in
        http*) : ;;
        *) form_url="$PORTAL_BASE$form_url" ;;
    esac
    # Django rotates the token on the form response, so the modal's hidden
    # field wins over the stale csrftoken cookie.
    csrf=${MODAL_CSRF:-$SESSION_CSRF}
    [[ -n $csrf ]] || die "the key creation form held no CSRF token"

    if [[ $(printf '%s\n' "${UNITS_TSV:-}" | grep -c . || true) -gt 1 ]]; then
        say "available units:"
        uchoice=$(pick_numbered_list "${UNITS_TSV}" "unit" 1)
        unit=$(printf '%s' "$uchoice" | cut -f1)
        label=$(printf '%s' "$uchoice" | cut -f2)
        sub "selected unit $label"
    else
        unit=$(printf '%s' "${UNITS_TSV:-}" | head -1 | cut -f1)
    fi

    name=$(ask_default_value "key name" "$DEFAULT_KEY_NAME")
    while :; do
        if [[ ! $name =~ ^[A-Za-z0-9_-]{1,15}$ ]]; then
            warn "the key name must be 1 to 15 characters from A-Z, a-z, 0-9, hyphen and underscore"
            name=$(ask_default_value "key name" "$name")
            continue
        fi
        if sqlite3 :memory: <<< "SELECT 1 FROM json_each('$(sqlq "$keys_json")') WHERE json_extract(value,'\$.name')='$name';" | grep -q 1; then
            local existing_id
            existing_id=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.id') FROM json_each('$(sqlq "$keys_json")') WHERE json_extract(value,'\$.name')='$name' AND COALESCE(json_extract(value,'\$.is_application'),0) = 0 AND COALESCE(json_extract(value,'\$.is_legacy'),0) = 0 AND COALESCE(json_extract(value,'\$.is_enabled'),0) = 1 AND COALESCE(json_extract(value,'\$.is_pending'),0) = 0 LIMIT 1;")
            if [[ -n $existing_id ]] && ask_yn "a usable key named $name already exists, reuse it?"; then
                REVEAL_KEY_ID=$existing_id
                return 0
            fi
            local suggestion
            suggestion=$(suggest_key_name "$keys_json")
            say "name $name is taken, next free suggestion: $suggestion"
            name=$(ask_default_value "key name" "$suggestion")
            continue
        fi
        say "creating key $name on the portal"
        resp=$(curl -sS --max-time "$HTTP_TIMEOUT" -o "$CONFDIR/.create.html" -w '%{http_code} %{url_effective}' -X POST "$form_url" \
            -H "Cookie: $SESSION_HEADER" -H "Referer: $form_url" -H "HX-Request: true" \
            -H "Content-Type: application/x-www-form-urlencoded" -H "User-Agent: $PORTAL_UA" \
            --data-urlencode "csrfmiddlewaretoken=$csrf" \
            --data-urlencode "name=$name" \
            --data-urlencode "unit=$unit" \
            --data "pool=BASIC" \
            --data "usage=PERSONAL") || die "could not submit the key creation form"
        code=${resp%% *}
        final_url=${resp#* }
        body=$(cat "$CONFDIR/.create.html")
        rm -f "$CONFDIR/.create.html"
        case $code in
            401 | 403) die "the portal session or CSRF token expired, rerun the installer" ;;
            5*) die "the portal had a server error creating the key, retry later" ;;
        esac
        case $final_url in
            "$PORTAL_BASE/aiaas"*) : ;;
            *) die "the portal redirected away during key creation, sign in again and rerun" ;;
        esac
        if printf '%s' "$body" | grep -qiE 'errorlist|already exists|invalid'; then
            local suggestion
            suggestion=$(suggest_key_name "$keys_json")
            warn "the portal refused the key name $name, next free suggestion: $suggestion"
            name=$(ask_default_value "key name" "$suggestion")
            continue
        fi
        say "key $name created on the portal, reading its secret"
        hdr_out=$(mktemp)
        keys_json=$(fetch_keys_json "$hdr_out")
        rm -f "$hdr_out"
        REVEAL_KEY_ID=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.id') FROM json_each('$(sqlq "$keys_json")') WHERE json_extract(value,'\$.name')='$name' AND COALESCE(json_extract(value,'\$.is_application'),0) = 0 AND COALESCE(json_extract(value,'\$.is_legacy'),0) = 0 AND COALESCE(json_extract(value,'\$.is_enabled'),0) = 1 AND COALESCE(json_extract(value,'\$.is_pending'),0) = 0 LIMIT 1;")
        [[ -n $REVEAL_KEY_ID && $REVEAL_KEY_ID != "null" ]] || die "the created key $name did not appear in the key list"
        return 0
    done
}

pick_numbered_list() {
    local tsv=$1 what=$2 default=$3 i n
    n=$(printf '%s\n' "$tsv" | grep -c . || true)
    for ((i = 1; i <= n; i++)); do
        printf '  %d) %s\n' "$i" "$(printf '%s\n' "$tsv" | sed -n "${i}p" | cut -f2)" >&2
    done
    local reply choice=""
    while :; do
        printf 'select %s [%s]:\n' "$what" "$default" >&2
        read -r reply || die "aborted"
        reply=${reply:-$default}
        if [[ $reply =~ ^[0-9]+$ ]] && ((reply >= 1 && reply <= n)); then
            break
        fi
        warn "pick a number between 1 and $n"
    done
    choice=$(printf '%s\n' "$tsv" | sed -n "${reply}p")
    printf '%s\n' "$choice"
}

ask_default_value() {
    local what=$1 default=$2 reply=""
    printf '%s [%s]:\n' "$what" "$default" >&2
    read -r reply || die "aborted"
    reply=${reply#"${reply%%[![:space:]]*}"}
    reply=${reply%"${reply##*[![:space:]]}"}
    printf '%s\n' "${reply:-$default}"
}

rcp_key_step() {
    step "[4/5] Selecting an RCP AIaaS key"
    if [[ -n $MANUAL_TOKEN ]]; then
        sub "using the key secret pasted during sign-in"
        API_TOKEN=$MANUAL_TOKEN
        return 0
    fi
    local hdr_out keys_json openwork_id
    hdr_out=$(mktemp)
    keys_json=$(fetch_keys_json "$hdr_out")
    rm -f "$hdr_out"
    openwork_id=$(sqlite3 :memory: <<< "SELECT json_extract(value,'\$.id') FROM json_each('$(sqlq "$keys_json")') WHERE json_extract(value,'\$.name')='$DEFAULT_KEY_NAME' AND COALESCE(json_extract(value,'\$.is_application'),0) = 0 AND COALESCE(json_extract(value,'\$.is_legacy'),0) = 0 AND COALESCE(json_extract(value,'\$.is_enabled'),0) = 1 AND COALESCE(json_extract(value,'\$.is_pending'),0) = 0 LIMIT 1;")
    if [[ -n $openwork_id && $openwork_id != "null" ]]; then
        sub "reusing the existing key named $DEFAULT_KEY_NAME"
        reveal_secret "$openwork_id"
        return 0
    fi
    local tsv count choice line id name pool label token
    tsv=$(keys_selectable_tsv "$keys_json")
    count=$(printf '%s\n' "$tsv" | grep -c . || true)
    if ((count > 0)); then
        say "existing API keys:"
        printf '%s\n' "$tsv" | awk -F'\t' '{ p = (tolower($3) == "premium") ? "premium" : "basic"; print "  " NR ") " $2 " (" p ")" }'
        say "  n) create a new key"
        local reply
        printf 'select a key or n [new]:\n' >&2
        read -r reply || die "aborted"
        if [[ $reply == [Nn]* ]]; then
            create_key_flow "$keys_json"
            reveal_secret "$REVEAL_KEY_ID"
            return 0
        fi
        if [[ $reply =~ ^[0-9]+$ ]] && ((reply >= 1 && reply <= count)); then
            line=$(printf '%s\n' "$tsv" | sed -n "${reply}p")
            id=$(printf '%s' "$line" | cut -f1)
            name=$(printf '%s' "$line" | cut -f2)
            pool=$(printf '%s' "$line" | cut -f3)
            label="basic"
            [[ $(printf '%s' "$pool" | tr '[:upper:]' '[:lower:]') == "premium" ]] && label="premium"
            sub "using key $name ($label)"
            reveal_secret "$id"
            return 0
        fi
        warn "no selection, creating a new key"
    fi
    create_key_flow "$keys_json"
    reveal_secret "$REVEAL_KEY_ID"
}

fresh_csrf_from_headers() {
    local v
    # The first Set-Cookie wins, like the Rust header parse.
    v=$(grep -i '^set-cookie:.*csrftoken=' "$1" | head -1 | sed -n 's/^[Ss]et-[Cc]ookie:.*csrftoken=\([^;]*\).*/\1/p' || true)
    if [[ -n $v ]]; then SESSION_CSRF=$v; fi
    true
}

write_provider_config() {
    step "[5/5] Writing the provider configuration"
    write_opencode_config "$API_TOKEN"
    write_env_store "$API_TOKEN"
    write_runtime_rows
    configure_workspace
    disable_telemetry
}

smoke_test() {
    local bin out errf rc
    if ! bin=$(locate_openwork_engine); then
        warn "smoke test skipped: OpenWork-managed engine not found"
        return 0
    fi
    say "running the smoke test: $OC_MODEL_PROVIDER answers one prompt"
    sub "engine: $bin"
    errf=$(mktemp)
    set +e
    out=$(EPFL_AIAAS_API_KEY="$API_TOKEN" run_with_timeout "$SMOKE_TIMEOUT" "$bin" run --standalone --model "$OC_MODEL_PROVIDER" "$SMOKE_PROMPT" < /dev/null 2> "$errf")
    rc=$?
    set -e
    if [[ $rc -eq 0 && -n ${out//[[:space:]]/} ]]; then
        local detail=${out}
        ((${#detail} > 500)) && detail="${detail:0:500}..."
        sub "$OC_MODEL_PROVIDER answered: $detail"
        rm -f "$errf"
        return 0
    fi
    warn "smoke test failed (exit code $rc)"
    # stderr first, then stdout, like the GUI's meaningful_lines.
    { cat "$errf"; printf '%s\n' "$out"; } | grep -v '^[[:space:]]*$' | head -10 | sed 's/^/    /' || true
    rm -f "$errf"
    warn "runtime error, see the output above (auth, model or network)"
    return 1
}

print_summary() {
    local launch="open -a OpenWork"
    [[ $OS != "macos" ]] && launch="$HOME/.local/share/openwork/OpenWork.AppImage"
    say ""
    say "==> Summary"
    sub "OpenWork is installed and configured"
    sub "provider: RCP AI Inference as a Service ($PROVIDER_ID)"
    sub "default model: $OPENWORK_MODEL_LARGE (RCP AIaaS Large)"
    sub "configs: ~/.config/opencode/opencode.json and $CONFDIR"
    sub "log: $LOG_FILE"
    sub "launch OpenWork with: $launch"
}

main() {
    if [[ $# -gt 0 ]]; then
        die "this installer takes no arguments"
    fi
    if [[ ! -t 0 ]]; then
        printf 'This installer needs an interactive terminal (stdin is not a TTY).\n'
        printf 'Run it with:\n  bash <(curl -fsSL %s)\n' "$INSTALL_URL"
        exit 1
    fi
    OS=$(uname -s)
    ARCH=$(uname -m)
    case $OS in
        Darwin) OS="macos" ;;
        Linux) OS="linux" ;;
        *) die "unsupported operating system: $OS (Linux and macOS only)" ;;
    esac
    case $ARCH in
        x86_64 | arm64 | aarch64) [[ $ARCH == "arm64" ]] && ARCH="aarch64" ;;
        *) die "unsupported architecture: $ARCH" ;;
    esac
    CONFDIR="${XDG_CONFIG_HOME:-$HOME/.config}/openwork"
    mkdir -p "$CONFDIR"
    LOG_FILE="$CONFDIR/install.log"
    # GNU sed takes -u, BSD sed takes -l, to keep the masking filter line-buffered.
    # The filter is line oriented, so every prompt must end its line or the user
    # sees nothing until the next newline lands.
    sed_flags=(-E)
    [[ $OS == "macos" ]] && sed_flags=(-l "${sed_flags[@]}")
    [[ $OS == "linux" ]] && sed_flags=(-u "${sed_flags[@]}")
    exec > >(sed "${sed_flags[@]}" -e 's/sk-[A-Za-z0-9_-]{6,}/sk-***masked***/g; s/(sessionid=)[^;[:space:]]+/\1***masked***/g; s/(csrftoken=)[^;[:space:]]+/\1***masked***/g' | tee -a "$LOG_FILE") 2>&1
    chmod 600 "$LOG_FILE" 2> /dev/null || true
    trap cleanup EXIT
    trap 'exit 130' INT

    say "OpenWork terminal installer ($(date -u '+%Y-%m-%dT%H:%M:%SZ'))"
    step "[1/5] Checking prerequisites and network"
    check_prereqs
    check_network
    install_openwork
    portal_login
    rcp_key_step
    write_provider_config
    if ! smoke_test; then
        print_summary
        die "the smoke test failed, fix the cause above and rerun the installer"
    fi
    print_summary
    say "done"
}

cleanup() {
    [[ -n ${ENGINE_EXTRACT_DIR:-} ]] && rm -rf "$ENGINE_EXTRACT_DIR"
    __restore_tty
    exec 1>&- 2>&- || true
    sleep 0.2
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
    main "$@"
fi
