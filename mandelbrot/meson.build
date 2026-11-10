project(
    'fractal',
    'rust',
    version: '14.1',
    license: 'GPL-3.0-or-later',
    meson_version: '>= 1.4',
)

fs = import('fs')
gnome = import('gnome')
i18n = import('i18n')

base_id = 'org.tunaos.mandelbrot'
application_id = base_id

major_version = '14.1'
pre_release_version = ''

version = major_version
if pre_release_version != ''
    version += '.' + pre_release_version
endif
full_version = version

dependency('glib-2.0', version: '>= 2.82')  # update when changing gtk version
dependency('gio-2.0', version: '>= 2.82')  # always same version as glib
dependency('gtk4', version: '>= 4.20.2')
dependency('libadwaita-1', version: '>= 1.8.0')

# Please keep these dependencies sorted.
dependency('gstreamer-1.0', version: '>= 1.20')
dependency('gstreamer-app-1.0', version: '>= 1.20')
dependency('gstreamer-base-1.0', version: '>= 1.20')
dependency('gstreamer-pbutils-1.0', version: '>= 1.20')
dependency('gstreamer-play-1.0', version: '>= 1.20')
dependency('gstreamer-video-1.0', version: '>= 1.20')
dependency('gtksourceview-5', version: '>= 5.0.0')
dependency('glycin-2', version: '>= 2.0.0')
dependency('glycin-gtk4-2', version: '>= 2.0.0')
dependency('libwebp', version: '>= 1.0.0')
dependency('shumate-1.0', version: '>= 1.1.0')
dependency('sqlite3', version: '>= 3.24.0')

# When the `build-env` profile is used, we only want to set up the build
# environment for the sandbox, we will not try to compile the app, so we can
# remove some build steps.
build_env_only = get_option('profile') == 'build-env'

cargo = find_program('cargo', required: true)
cargo_version = run_command(cargo, '--version', check: true).stdout().strip()
message(cargo_version)
rustc_version = run_command('rustc', '--version', check: true).stdout().strip()
message(rustc_version)

if not build_env_only
    glib_compile_resources = find_program(
        'glib-compile-resources',
        required: true,
    )
    glib_compile_schemas = find_program('glib-compile-schemas', required: true)
    desktop_file_validate = find_program(
        'desktop-file-validate',
        required: false,
    )
    appstreamcli = find_program('appstreamcli', required: false)

    blueprint_compiler = find_program('blueprint-compiler', required: true)
    blueprint_compiler_version = run_command(
        'blueprint-compiler',
        '--version',
        check: true,
    ).stdout().strip()
    message('blueprint-compiler ' + blueprint_compiler_version)
endif

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
localedir = prefix / get_option('localedir')

datadir = prefix / get_option('datadir')
pkgdatadir = datadir / meson.project_name()
iconsdir = datadir / 'icons'
podir = meson.project_source_root() / 'po'
gettext_package = meson.project_name()

if get_option('profile') == 'development' or build_env_only
    profile = 'Devel'
    application_id += '.Devel'
elif get_option('profile') == 'hack'
    profile = 'Devel'
    application_id += '.Hack'
elif get_option('profile') == 'beta'
    profile = 'Beta'
else
    profile = 'Stable'
endif

cargo_target_dir = meson.project_build_root() / 'cargo-target'
cargo_home = meson.project_build_root() / 'cargo-home'

cargo_env = [
    'CARGO_HOME=' + cargo_home,
    'CARGO_TARGET_DIR=' + cargo_target_dir,
]

if profile == 'Devel'
    vcs_tag = run_command('git', 'rev-parse', '--short', 'HEAD', check: false).stdout().strip()
    if vcs_tag == ''
        devel_version = profile.to_lower()
    else
        devel_version = vcs_tag
    endif
    full_version += '-' + devel_version

    release_date = run_command('git', 'show', '-s', '--format=%cI', check: true).stdout().strip()

    # Setup pre-commit hook for ensuring coding style is always consistent
    message('Compiling pre-commit binary…')
    run_command(
        cargo,
        'build',
        '--manifest-path',
        'hooks/checks/Cargo.toml',
        check: true,
        env: cargo_env,
    )
    cp_bin_result = run_command(
        'cp',
        '-f',
        cargo_target_dir / 'debug/checks',
        'hooks/checks-bin',
        check: false,
    )
    cp_hook_result = run_command(
        'cp',
        '-f',
        'hooks/pre-commit.hook',
        '.git/hooks/pre-commit',
        check: false,
    )

    if cp_bin_result.returncode() == 0 and cp_hook_result.returncode() == 0
        message('Pre-commit hook installed')
    else
        if cp_bin_result.returncode() != 0
            message(
                'Could not install pre-commit binary: ' + cp_bin_result.stderr(),
            )
        endif
        if cp_hook_result.returncode() != 0
            message(
                'Could not install pre-commit hook: ' + cp_hook_result.stderr(),
            )
        endif
    endif
endif

# Generate nextest config file
# We generate it dynamically to put the test report inside the build directory,
# like all other artefacts
nextest_store_dir = meson.project_build_root() / 'nextest'
nextest_data = configuration_data()
nextest_data.set_quoted('STORE_DIR', nextest_store_dir)
nextest_config = configure_file(
    input: 'nextest.toml.in',
    output: 'nextest.toml',
    configuration: nextest_data,
)

if not build_env_only
    subdir('data')
    subdir('po')
endif

subdir('src')

gnome.post_install(
    gtk_update_icon_cache: true,
    glib_compile_schemas: true,
    update_desktop_database: true,
)
