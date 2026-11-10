if profile == 'Devel'
    icon_id = base_id + '.Devel'
else
    icon_id = base_id
endif

install_data(
    '@0@.svg'.format(icon_id),
    install_dir: iconsdir / 'hicolor' / 'scalable' / 'apps',
    rename: '@0@.svg'.format(application_id),
)

install_data(
    '@0@-symbolic.svg'.format(base_id),
    install_dir: iconsdir / 'hicolor' / 'symbolic' / 'apps',
    rename: '@0@-symbolic.svg'.format(application_id),
)
