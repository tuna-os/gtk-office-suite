subdir('icons')

# Desktop file
desktop_conf = configuration_data()
desktop_conf.set('icon', application_id)
desktop_file = i18n.merge_file(
    type: 'desktop',
    input: configure_file(
        input: '@0@.desktop.in.in'.format(base_id),
        output: '@BASENAME@',
        configuration: desktop_conf,
    ),
    output: '@0@.desktop'.format(application_id),
    po_dir: podir,
    install: true,
    install_dir: datadir / 'applications',
)
# Validate Desktop file
if desktop_file_validate.found()
    test(
        'validate-desktop',
        desktop_file_validate,
        args: [desktop_file.full_path()],
    )
endif

# Appdata
if profile == 'Devel'
    appstream_version = major_version
    if pre_release_version != ''
        appstream_version += '~' + pre_release_version
    endif
    appstream_version += '-' + devel_version

    development_release = '''
    <release version="@0@" type="snapshot" date="@1@">
      <description>
        <p>Development release.</p>
      </description>
    </release>'''.format(appstream_version, release_date)
else
    development_release = ''
endif
appdata_conf = configuration_data()
appdata_conf.set('app-id', application_id)
appdata_conf.set('gettext-package', gettext_package)
appdata_conf.set('development-release', development_release)
appdata_file = i18n.merge_file(
    input: configure_file(
        input: '@0@.metainfo.xml.in.in'.format(base_id),
        output: '@BASENAME@',
        configuration: appdata_conf,
    ),
    output: '@0@.metainfo.xml'.format(application_id),
    po_dir: podir,
    install: true,
    install_dir: datadir / 'metainfo',
)
# Validate Appdata
if appstreamcli.found()
    test(
        'validate-appdata',
        appstreamcli,
        args: ['validate', '--no-net', appdata_file.full_path()],
    )
endif

# GSchema
gschema_conf = configuration_data()
gschema_conf.set('app-id', application_id)
gschema_conf.set('profile', profile)
gschema_conf.set('gettext-package', gettext_package)
configure_file(
    input: '@0@.gschema.xml.in'.format(base_id),
    output: '@0@.gschema.xml'.format(application_id),
    configuration: gschema_conf,
    install: true,
    install_dir: datadir / 'glib-2.0' / 'schemas',
)
# Validate GSchema
if glib_compile_schemas.found()
    test(
        'validate-gschema',
        glib_compile_schemas,
        args: ['--strict', '--dry-run', meson.current_source_dir()],
    )
endif

# D-Bus service file
service_conf = configuration_data()
service_conf.set('application_id', application_id)
service_conf.set('bindir', bindir)
configure_file(
    input: 'org.tunaos.mandelbrot.service.in',
    output: '@0@.service'.format(application_id),
    configuration: service_conf,
    install: true,
    install_dir: datadir / 'dbus-1/services',
)

subdir('resources')
