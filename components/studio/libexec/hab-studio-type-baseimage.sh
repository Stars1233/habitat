# TESTING CHANGES
# Documentation on testing local changes to this lives here:
# https://github.com/habitat-sh/habitat/blob/master/BUILDING.md#testing-changes

# shellcheck disable=2034
studio_type="baseimage"
studio_path="$HAB_ROOT_PATH/bin"
studio_enter_environment=
studio_build_environment=
studio_build_command=
studio_run_environment=
studio_run_command=

base_pkgs="chef/hab chef/hab-launcher chef/hab-sup"
: "${PKGS:=}"

run_user="hab"
run_group="$run_user"

finish_setup() {
  if [ -h "$HAB_STUDIO_ROOT$HAB_ROOT_PATH/bin/hab" ]; then
    return 0
  fi

  for embed in $PKGS; do
    if [ -d "$HAB_PKG_PATH/$embed" ]; then
      echo "> Using local package for $embed"
      embed_path=$(_outside_pkgpath_for $embed)
      # shellcheck disable=2154
      $bb mkdir -p "$HAB_STUDIO_ROOT"/"$embed_path"
      $bb cp -ra "$embed_path"/* "$HAB_STUDIO_ROOT"/"$embed_path"
      for tdep in $($bb cat "$embed_path"/TDEPS); do
        echo "> Using local package for $tdep via $embed"
        $bb mkdir -p "$HAB_STUDIO_ROOT""$HAB_PKG_PATH"/"$tdep"
        $bb cp -ra "$HAB_PKG_PATH"/"$tdep"/* "$HAB_STUDIO_ROOT""$HAB_PKG_PATH"/"$tdep"
      done
    else
      _hab pkg install $embed
    fi
  done

  for pkg in $base_pkgs; do
    _hab pkg install "$pkg"
  done

  local hab_path
  hab_path=$(_pkgpath_for chef/hab)
  local sup_path
  sup_path=$(_pkgpath_for chef/hab-sup)
  local busybox_path
  busybox_path=$(_pkgpath_for core/busybox-static)

  local full_path=""
  for path_pkg in $PKGS chef/hab-launcher chef/hab-sup core/busybox-static; do
    local path_file
    path_file="$HAB_STUDIO_ROOT/$(_pkgpath_for $path_pkg)/PATH"
    if [ -f "$path_file" ]; then
      if [ -z "$full_path" ]; then
        full_path="$($bb cat "$path_file")"
      else
        full_path="$full_path:$($bb cat "$path_file")"
      fi
    fi

    local tdeps_file
    tdeps_file="$HAB_STUDIO_ROOT/$(_pkgpath_for $path_pkg)/TDEPS"
    if [ -f "$tdeps_file" ]; then
      for tdep in $($bb cat "$tdeps_file"); do
        local tdep_path_file
        tdep_path_file="$HAB_STUDIO_ROOT/$(_pkgpath_for "$tdep")/PATH"
        if [ -f "$tdep_path_file" ]; then
          full_path="$full_path:$($bb cat "$tdep_path_file")"
        fi
      done
    fi
  done
  full_path="$full_path:$HAB_ROOT_PATH/bin"

  studio_path="$full_path"
  studio_enter_command="${busybox_path}/bin/sh --login"

  # shellcheck disable=2086,2154
  $bb mkdir -p $v "$HAB_STUDIO_ROOT""$HAB_ROOT_PATH"/bin

  # Put `hab` on the default `$PATH`
  _hab pkg binlink --dest "$HAB_ROOT_PATH"/bin chef/hab hab

  # Create `/bin/{sh,bash}` for software that hardcodes these shells
  _hab pkg binlink --dest=/bin core/busybox-static bash
  _hab pkg binlink --dest=/bin core/busybox-static sh

  # Set the login shell for any relevant user to be `/bin/bash`
  $bb sed -e "s,/bin/sh,$busybox_path/bin/bash,g" -i "$HAB_STUDIO_ROOT"/etc/passwd

  $bb cat <<PROFILE > "$HAB_STUDIO_ROOT"/etc/profile
# Add hab to the default \$PATH at the front so any wrapping scripts will
# be found and called first
export PATH=$full_path:\$PATH

# Colorize grep/egrep/fgrep by default
alias grep='grep --color=auto'
alias egrep='egrep --color=auto'
alias fgrep='fgrep --color=auto'

# Add command line completion
source <(hab cli completers --shell bash)
PROFILE

  $bb cat <<EOT > "$HAB_STUDIO_ROOT"/etc/resolv.conf
nameserver 8.8.8.8
nameserver 8.8.4.4
EOT

  $bb cat <<EOT > "$HAB_STUDIO_ROOT"/etc/nsswitch.conf
passwd:     files
group:      files
shadow:     files

hosts:      files dns
networks:   files

rpc:        files
services:   files
EOT
  echo "${run_user}:x:42:42:root:/:/bin/sh" >> "$HAB_STUDIO_ROOT"/etc/passwd
  echo "${run_group}:x:42:${run_user}" >> "$HAB_STUDIO_ROOT"/etc/group

  local sup="$HAB_ROOT_PATH/bin/hab sup"
  $bb touch "$HAB_STUDIO_ROOT"/.hab_pkg
  $bb cat <<EOT > "$HAB_STUDIO_ROOT"/init.sh
#!$busybox_path/bin/sh
export PATH=$full_path
case \$1 in
  -h|--help|help|-V|--version) exec $sup "\$@";;
  -*) exec $sup start \$(cat /.hab_pkg) "\$@";;
  *) exec $sup "\$@";;
esac
EOT
  $bb chmod a+x "$HAB_STUDIO_ROOT"/init.sh

  studio_env_command="$busybox_path/bin/env"
}

# Intentionally using a subshell here so `unset` doesn't affect the
# caller's environment.
_hab() (
    unset HAB_CACHE_KEY_PATH
    # shellcheck disable=2154
    # Set the HAB_LICENSE because the license accepted files don't yet exist on the chroot filesystem
    $bb env FS_ROOT="$HAB_STUDIO_ROOT" HAB_LICENSE="$HAB_LICENSE" "$hab" "$@"
)

_pkgpath_for() {
  _hab pkg path "$1" | $bb sed -e "s,^$HAB_STUDIO_ROOT,,g"
}

_outside_pkgpath_for() {
  HAB_LICENSE="$HAB_LICENSE" $hab pkg path "$1"
}
