if not build_env_only
    # Compile Blueprint resources
    blp_files = []
    ui_files = []
    ui_files_xml = ''

    # These files are already added manually in the resources file because they
    # need custom handling.
    blp_gresource_exceptions = [
        'shortcuts-dialog.blp',  # Must use the root prefix for auto-detection, it must not be under `/ui`.
    ]

    foreach file : fs.read('ui-blueprint-resources.in').splitlines()
        file = file.strip()

        if file.startswith('#') or file == ''
            continue
        endif

        blp_files += file

        # We need to put all output files in the same directory
        ui_alias = file.replace('.blp', '.ui')
        ui_file = ui_alias.replace('/', '-')
        ui_files += ui_file

        if not blp_gresource_exceptions.contains(file)
            ui_files_xml += '\n    <file compressed="true" preprocess="xml-stripblanks" alias="' + ui_alias + '">' + ui_file + '</file>'
        endif
    endforeach

    blueprints = custom_target(
        'blueprints',
        input: blp_files,
        output: ui_files,
        command: [
            '../build-aux/compile-blueprints.sh',
            blueprint_compiler.full_path(),
            '@OUTDIR@',
            '@CURRENT_SOURCE_DIR@',
            '@INPUT@',
        ],
    )

    # Populate the GResource file for the compiled Blueprint files dynamically.
    ui_resources_xml = configure_file(
        input: 'ui-resources.gresource.xml.in',
        output: 'ui-resources.gresource.xml',
        configuration: {'UI_FILES': ui_files_xml},
    )

    # Compile the UI files.
    ui_resources = gnome.compile_resources(
        'ui-resources',
        ui_resources_xml,
        gresource_bundle: true,
        install: true,
        install_dir: pkgdatadir,
        dependencies: [blueprints, ui_resources_xml],
    )
endif

# Generate config.rs
global_conf = configuration_data()
global_conf.set_quoted('APP_ID', application_id)
global_conf.set(
    'DISABLE_GLYCIN_SANDBOX',
    get_option('disable-glycin-sandbox').to_string(),
)
global_conf.set_quoted('GETTEXT_PACKAGE', gettext_package)
global_conf.set_quoted('LOCALEDIR', localedir)
global_conf.set_quoted('PKGDATADIR', pkgdatadir)
global_conf.set('PROFILE', profile)
global_conf.set_quoted('VERSION', full_version)
global_conf.set_quoted('BUILD_DIR', meson.project_build_root())
config = configure_file(
    input: 'config.rs.in',
    output: 'config.rs',
    configuration: global_conf,
)
run_command(
    'cp',
    config,
    meson.project_source_root() / 'src' / 'config.rs',
    check: true,
)

# Cargo settings
cargo_options = ['--manifest-path', meson.project_source_root() / 'Cargo.toml']

if get_option('calls-media')
    cargo_options += ['--features', 'calls-media']
    message('Native call media (LiveKit) enabled')
endif

# `--locked` for the binary we ship: without it an incomplete Cargo.lock is
# silently topped up from the registry at build time, so the Flatpak build
# resolves whatever is newest instead of what was tested. That is how the
# livekit-api 0.5.6 / livekit-protocol 0.7.12 mismatch got in.
cargo_build_options = cargo_options + ['--locked']

if profile == 'Devel'
    rust_target = 'debug'
    message('Building in debug mode')
else
    cargo_build_options += ['--release']
    rust_target = 'release'
    message('Building in release mode')
endif

if not build_env_only
    # Build binary with cargo
    custom_target(
        'cargo-build',
        build_by_default: true,
        build_always_stale: true,
        output: meson.project_name(),
        console: true,
        install: true,
        install_dir: bindir,
        depends: [resources, ui_resources],
        env: cargo_env,
        command: [
            cargo,
            'build',
            cargo_build_options,
            '&&',
            'cp',
            cargo_target_dir / rust_target / meson.project_name(),
            '@OUTPUT@',
        ],
    )

    # Lint Rust code with clippy
    run_target(
        'cargo-clippy',
        env: cargo_env,
        command: [
            cargo,
            'clippy',
            cargo_options,
        ],
    )

    # Run Rust tests with cargo-nextest
    custom_target(
        'cargo-test',
        build_always_stale: true,
        output: 'junit.xml',
        console: true,
        depends: [resources, ui_resources],
        command: [
            'env',
            cargo_env,
            'cargo-nextest',
            'nextest',
            'run',
            cargo_options,
            '--config-file',
            nextest_config.full_path(),
            '&&',
            'cp',
            nextest_store_dir / 'default/junit.xml',
            '@OUTPUT@',
        ],
    )
endif

# Build docs with rustdoc
rustdoc_flags = ' '.join(
    '-Zunstable-options',
    '--enable-index-page',
    # The gtk-rs project docs on docs.rs don't build or don't have comments.
    '--extern-html-root-url=gdk4=https://gtk-rs.org/gtk4-rs/stable/latest/docs/',
    '--extern-html-root-url=gio=https://gtk-rs.org/gtk-rs-core/stable/latest/docs/',
    '--extern-html-root-url=glib=https://gtk-rs.org/gtk-rs-core/stable/latest/docs/',
    '--extern-html-root-url=gsk4=https://gtk-rs.org/gtk4-rs/stable/latest/docs/',
    '--extern-html-root-url=gtk4=https://gtk-rs.org/gtk4-rs/stable/latest/docs/',
    '--extern-html-root-url=libadwaita=https://world.pages.gitlab.gnome.org/Rust/libadwaita-rs/stable/latest/docs/',
    # We are usually on a commit rather than on a stable release, so we are more likely to have up-to-date comments on nightly docs.
    '--extern-html-root-url=matrix-sdk=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-base=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-common=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-crypto=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-qrcode=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-sqlite=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-store-encryption=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-ui=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=matrix-sdk-ui=https://matrix-org.github.io/matrix-rust-sdk/',
    '--extern-html-root-url=ruma=https://docs.ruma.dev/',
    '--extern-html-root-url=ruma-client-api=https://docs.ruma.dev/',
    '--extern-html-root-url=ruma-common=https://docs.ruma.dev/',
    '--extern-html-root-url=ruma-events=https://docs.ruma.dev/',
    '--extern-html-root-url=ruma-html=https://docs.ruma.dev/',
    '--extern-html-root-url=ruma-signatures=https://docs.ruma.dev/',
    '--cfg=docsrs',
    '-Dwarnings',
)
cargo_doc_env = ['RUSTDOCFLAGS=' + rustdoc_flags]
cargo_doc_options = ['--package=fractal', '--no-deps', '-Zrustdoc-map']

custom_target(
    'cargo-doc',
    build_always_stale: true,
    output: 'doc',
    console: true,
    env: cargo_env + cargo_doc_env,
    command: [
        cargo,
        'doc',
        cargo_options + cargo_doc_options,
        '&&',
        'cp',
        '-r',
        cargo_target_dir / 'doc',
        '@OUTPUT@',
    ],
)
