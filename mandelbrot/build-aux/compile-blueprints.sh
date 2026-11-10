#!/usr/bin/env bash
#
# Compile blueprint files to UI files all in the same directory.
#
# Usage: ./compile-blueprints.sh PATH_TO_BLUEPRINT_COMPILER OUTPUT_DIR BASE_INPUT_DIR [INPUT_FILE…]
#
# This should be a temporary solution to fix an incompatibility between our
# setup, blueprint-compiler and meson.
#
# The problem is that blueprint advises to use `custom_target()` with `output`
# set to `.`: https://gitlab.gnome.org/GNOME/blueprint-compiler/-/blob/70f32dc069c3ff13a54f24f24c0e720b938f587d/docs/setup.rst#L55
# This doesn't work well with ninja's caching, as it doesn't know which files
# are actually generated, so it might consider that the output is generated too
# soon, and proceed to compiling the gresource before all blueprint files are
# actually compiled.
#
# The fix for this is to actually set the proper list of generated files for the
# output. This doesn't work with our setup because we generate files in
# subdirectories and `custom_target`'s `output` only accepts a list of files in
# the current directory.
#
# So we have 2 options to fix this:
#
# 1. Use a `meson.build` file per subdirectory to compile the blueprints
#    separately which is tedious and results in a lot of duplication.
# 2. Keep our current setup and find a way to generate a list of outputs that is
#    accepted by meson.
#
# For simplicity, we want to use the second option and it would be easy if we
# could use `generator()` to compile the blueprints. It requires
# `gnome.compile_resources()` to accept a `generated_list` in `dependencies`,
# but it doesn't: https://github.com/mesonbuild/meson/issues/12336.
#
# So this implements another solution: compile all the UI files in the same
# directory by using a name scheme that avoids collisions. Then we can overwrite
# the path where the resource will be available in the gresource file
# definition by setting an `alias`.
#
# The output files will be located in `OUTPUT_DIR` and their names are the
# path of `INPUT_FILE` relative to the `BASE_INPUT_DIR` with the slashes
# replaced by `-`, and the extension changed.

set -e

compiler="$1"
shift
output_dir="$1"
shift
base_input_dir="$1"
shift

# For debugging.
# echo "Compiling files in $input_dir to $output_dir with $compiler"

for input_file in "$@"
do
    # Change extension.
    output_file="${input_file%.blp}.ui"
    # Remove base input dir to get relative path.
    output_file="${output_file#$base_input_dir\/}"
    # Replace slashes with dashes.
    output_file="$output_dir/${output_file//\//-}"

    # For debugging
    # echo "Compiling $input_file to $output_file"

    "$compiler" compile --output "$output_file" "$input_file"
done
