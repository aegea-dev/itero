#!/bin/sh
# Builds each dev container in .devcontainer/ with the dev container CLI (the
# one Codespaces and VS Code use), as a newcomer or a contributor would open
# it, and checks it from inside (scripts/devcontainer-check.sh). Not in CI:
# it downloads the base image, Chromium and the latest itero release.
# Needs Docker and npx. Removes the containers it made.
set -eu
cli="npx -y @devcontainers/cli@0.89.0"
root=$(pwd)
done_with() {
	ids=$(docker ps -aq --filter "label=devcontainer.local_folder=$root")
	[ -z "$ids" ] || docker rm -f $ids >/dev/null
}
trap done_with EXIT
# try is .devcontainer/devcontainer.json, the one opened first; develop is
# in its folder
for c in try develop; do
	config="$root/.devcontainer/devcontainer.json"
	[ "$c" = try ] || config="$root/.devcontainer/$c/devcontainer.json"
	echo "== $c"
	$cli up --workspace-folder "$root" --config "$config" --remove-existing-container
	$cli exec --workspace-folder "$root" --config "$config" sh scripts/devcontainer-check.sh "$c"
	done_with
done
