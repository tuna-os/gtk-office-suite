#![allow(clippy::too_many_lines)] // We do not care if tests are too long.

use std::{cell::RefCell, rc::Rc};

use gtk::{gio, glib, glib::clone, prelude::*};
use sourceview::prelude::ListModelExt;

use super::{GroupingListGroup, GroupingListModel};
use crate::utils::PlaceholderObject;

#[test]
fn only_singletons_single_item_changes() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    let model = GroupingListModel::new(|_lhs, _rhs| false);

    assert_eq!(model.n_items(), 0);

    let items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        items_changed,
        move |_, position, removed, added| {
            println!(
                "connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("item 0.0"),
            PlaceholderObject::new("item 1.0"),
            PlaceholderObject::new("item 2.0"),
            PlaceholderObject::new("item 3.0"),
            PlaceholderObject::new("item 4.0"),
        ],
    );

    assert_eq!(items_changed.take(), Some((0, 0, 5)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    // Remove the first item.
    list_store.remove(0);

    assert_eq!(items_changed.take(), Some((0, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    // Remove the last item.
    list_store.remove(3);

    assert_eq!(items_changed.take(), Some((3, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Remove the item in the middle.
    list_store.remove(1);

    assert_eq!(items_changed.take(), Some((1, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Re-add first item.
    list_store.insert(0, &PlaceholderObject::new("item 0.1"));

    assert_eq!(items_changed.take(), Some((0, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Re-add middle item.
    list_store.insert(2, &PlaceholderObject::new("item 2.1"));

    assert_eq!(items_changed.take(), Some((2, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Re-add end item.
    list_store.insert(4, &PlaceholderObject::new("item 4.1"));

    assert_eq!(items_changed.take(), Some((4, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace first item.
    list_store.splice(0, 1, &[PlaceholderObject::new("item 0.2")]);

    assert_eq!(items_changed.take(), Some((0, 1, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace second item.
    list_store.splice(1, 1, &[PlaceholderObject::new("item 1.1")]);

    assert_eq!(items_changed.take(), Some((1, 1, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace last item.
    list_store.splice(4, 1, &[PlaceholderObject::new("item 4.2")]);

    assert_eq!(items_changed.take(), Some((4, 1, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.2"
    );
}

#[test]
fn only_singletons_multiple_item_changes() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    let model = GroupingListModel::new(|_lhs, _rhs| false);

    assert_eq!(model.n_items(), 0);

    let items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        items_changed,
        move |_, position, removed, added| {
            println!(
                "connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("item 0.0"),
            PlaceholderObject::new("item 1.0"),
            PlaceholderObject::new("item 2.0"),
            PlaceholderObject::new("item 3.0"),
            PlaceholderObject::new("item 4.0"),
        ],
    );

    assert_eq!(items_changed.take(), Some((0, 0, 5)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    // Remove the 2 first items.
    list_store.splice(0, 2, &[] as &[glib::Object]);

    assert_eq!(items_changed.take(), Some((0, 2, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    // Remove the 2 last items.
    list_store.splice(1, 2, &[] as &[glib::Object]);

    assert_eq!(items_changed.take(), Some((1, 2, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );

    // Re-add the 2 first items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("item 0.1"),
            PlaceholderObject::new("item 1.1"),
        ],
    );

    assert_eq!(items_changed.take(), Some((0, 0, 2)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );

    // Re-add the 2 last items.
    list_store.splice(
        3,
        0,
        &[
            PlaceholderObject::new("item 3.1"),
            PlaceholderObject::new("item 4.1"),
        ],
    );

    assert_eq!(items_changed.take(), Some((3, 0, 2)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.1"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Remove the 3 middle items.
    list_store.splice(1, 3, &[] as &[glib::Object]);

    assert_eq!(items_changed.take(), Some((1, 3, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Re-add the 3 middle items.
    list_store.splice(
        1,
        0,
        &[
            PlaceholderObject::new("item 1.2"),
            PlaceholderObject::new("item 2.1"),
            PlaceholderObject::new("item 3.2"),
        ],
    );

    assert_eq!(items_changed.take(), Some((1, 0, 3)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.2"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.2"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace the 2 first items.
    list_store.splice(
        0,
        2,
        &[
            PlaceholderObject::new("item 0.2"),
            PlaceholderObject::new("item 1.3"),
        ],
    );

    assert_eq!(items_changed.take(), Some((0, 2, 2)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.3"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.2"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace 2 middle items.
    list_store.splice(
        2,
        2,
        &[
            PlaceholderObject::new("item 2.2"),
            PlaceholderObject::new("item 3.3"),
        ],
    );

    assert_eq!(items_changed.take(), Some((2, 2, 2)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.3"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.2"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.3"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace the 2 last items.
    list_store.splice(
        3,
        2,
        &[
            PlaceholderObject::new("item 3.4"),
            PlaceholderObject::new("item 4.2"),
        ],
    );

    assert_eq!(items_changed.take(), Some((3, 2, 2)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.3"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.2"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.4"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.2"
    );

    // Remove all the items.
    list_store.remove_all();

    assert_eq!(items_changed.take(), Some((0, 5, 0)));

    assert_eq!(model.n_items(), 0);
}

#[test]
fn only_group_single_item_changes() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    let model = GroupingListModel::new(|_lhs, _rhs| true);

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("item 0.0"),
            PlaceholderObject::new("item 1.0"),
            PlaceholderObject::new("item 2.0"),
            PlaceholderObject::new("item 3.0"),
            PlaceholderObject::new("item 4.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 1);
    let group = items[0].downcast_ref::<GroupingListGroup>().unwrap();

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.0"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    let group_items_changed = Rc::new(RefCell::new(None));

    group.connect_items_changed(clone!(
        #[strong]
        group_items_changed,
        move |_, position, removed, added| {
            println!(
                "group.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                group_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    // Remove the first item.
    list_store.remove(0);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((0, 1, 0)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 4);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    // Remove the last item.
    list_store.remove(3);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((3, 1, 0)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 3);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Remove the item in the middle.
    list_store.remove(1);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((1, 1, 0)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 2);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Re-add first item.
    list_store.insert(0, &PlaceholderObject::new("item 0.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((0, 0, 1)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 3);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Re-add middle item.
    list_store.insert(2, &PlaceholderObject::new("item 2.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((2, 0, 1)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 4);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );

    // Re-add end item.
    list_store.insert(4, &PlaceholderObject::new("item 4.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((4, 0, 1)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace first item.
    list_store.splice(0, 1, &[PlaceholderObject::new("item 0.2")]);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((0, 1, 1)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace second item.
    list_store.splice(1, 1, &[PlaceholderObject::new("item 1.1")]);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((1, 1, 1)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace last item.
    list_store.splice(4, 1, &[PlaceholderObject::new("item 4.2")]);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((4, 1, 1)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.2"
    );
}

#[test]
fn only_group_multiple_item_changes() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    let model = GroupingListModel::new(|_lhs, _rhs| true);

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("item 0.0"),
            PlaceholderObject::new("item 1.0"),
            PlaceholderObject::new("item 2.0"),
            PlaceholderObject::new("item 3.0"),
            PlaceholderObject::new("item 4.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 1);
    let group = items[0].downcast_ref::<GroupingListGroup>().unwrap();

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.0"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    let group_items_changed = Rc::new(RefCell::new(None));

    group.connect_items_changed(clone!(
        #[strong]
        group_items_changed,
        move |_, position, removed, added| {
            println!(
                "group.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                group_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    // Remove the 2 first items.
    list_store.splice(0, 2, &[] as &[glib::Object]);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((0, 2, 0)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 3);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    // Re-add the 2 first items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("item 0.1"),
            PlaceholderObject::new("item 1.1"),
        ],
    );

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((0, 0, 2)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.0"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.0"
    );

    // Remove the 2 last items.
    list_store.splice(3, 2, &[] as &[glib::Object]);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((3, 2, 0)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 3);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );

    // Re-add the 2 last items.
    list_store.splice(
        3,
        0,
        &[
            PlaceholderObject::new("item 3.1"),
            PlaceholderObject::new("item 4.1"),
        ],
    );

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((3, 0, 2)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.1"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.0"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.1"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Remove the 3 middle items.
    list_store.splice(1, 3, &[] as &[glib::Object]);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((1, 3, 0)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 2);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Re-add the 3 middle items.
    list_store.splice(
        1,
        0,
        &[
            PlaceholderObject::new("item 1.2"),
            PlaceholderObject::new("item 2.1"),
            PlaceholderObject::new("item 3.2"),
        ],
    );

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((1, 0, 3)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.1"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.2"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.2"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace the 2 first items.
    list_store.splice(
        0,
        2,
        &[
            PlaceholderObject::new("item 0.2"),
            PlaceholderObject::new("item 1.3"),
        ],
    );

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((0, 2, 2)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.3"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.1"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.2"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace 2 middle items.
    list_store.splice(
        2,
        2,
        &[
            PlaceholderObject::new("item 2.2"),
            PlaceholderObject::new("item 3.3"),
        ],
    );

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((2, 2, 2)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.3"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.2"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.3"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.1"
    );

    // Replace the 2 last items.
    list_store.splice(
        3,
        2,
        &[
            PlaceholderObject::new("item 3.4"),
            PlaceholderObject::new("item 4.2"),
        ],
    );

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 1);

    assert_eq!(group_items_changed.take(), Some((3, 2, 2)));

    let group_items = group.snapshot();
    assert_eq!(group_items.len(), 5);
    assert_eq!(
        group_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 0.2"
    );
    assert_eq!(
        group_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 1.3"
    );
    assert_eq!(
        group_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 2.2"
    );
    assert_eq!(
        group_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 3.4"
    );
    assert_eq!(
        group_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "item 4.2"
    );

    // Remove all the items.
    list_store.remove_all();

    assert_eq!(model_items_changed.take(), Some((0, 1, 0)));
    assert_eq!(model.n_items(), 0);

    assert_eq!(group_items_changed.take(), None);
}

#[test]
fn mixed_with_group_at_start() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
            PlaceholderObject::new("group 1 - item 2.0"),
            PlaceholderObject::new("singleton 1.0"),
            PlaceholderObject::new("singleton 2.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 3)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    let group1 = items[0].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));

    group1.connect_items_changed(clone!(
            #[strong]
            group1_items_changed,
            move |_, position, removed, added| {
                println!(
                    "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
                );
                assert_eq!(
                    group1_items_changed.replace(Some((position, removed, added))),
                    None
                );
            }
        ));

    // Remove the first item in the group.
    list_store.remove(0);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((0, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(group1.n_items(), 2);

    // Re-add the first item in the group.
    list_store.insert(0, &PlaceholderObject::new("group 1 - item 0.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((0, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Remove the middle item in the group.
    list_store.remove(1);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((1, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Re-add the middle item in the group.
    list_store.insert(1, &PlaceholderObject::new("group 1 - item 1.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((1, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Remove the last item in the group.
    list_store.remove(2);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((2, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );

    // Re-add the last item in the group.
    list_store.insert(2, &PlaceholderObject::new("group 1 - item 2.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((2, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.1"
    );

    // Remove group and first singleton.
    list_store.splice(0, 4, &[] as &[glib::Object]);

    assert_eq!(model_items_changed.take(), Some((0, 2, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    assert_eq!(group1_items_changed.take(), None);
}

#[test]
fn mixed_with_group_in_middle() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("singleton 0.0"),
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
            PlaceholderObject::new("group 1 - item 2.0"),
            PlaceholderObject::new("singleton 2.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 3)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    let group1 = items[1].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));

    group1.connect_items_changed(clone!(
            #[strong]
            group1_items_changed,
            move |_, position, removed, added| {
                println!(
                    "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
                );
                assert_eq!(
                    group1_items_changed.replace(Some((position, removed, added))),
                    None
                );
            }
        ));

    // Remove the first item in the group.
    list_store.remove(1);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((0, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Re-add the first item in the group.
    list_store.insert(1, &PlaceholderObject::new("group 1 - item 0.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((0, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Remove the middle item in the group.
    list_store.remove(2);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((1, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Re-add the middle item in the group.
    list_store.insert(2, &PlaceholderObject::new("group 1 - item 1.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((1, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Remove the last item in the group.
    list_store.remove(3);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((2, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );

    // Re-add the last item in the group.
    list_store.insert(3, &PlaceholderObject::new("group 1 - item 2.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((2, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.1"
    );

    // Remove group.
    list_store.splice(1, 3, &[] as &[glib::Object]);

    assert_eq!(model_items_changed.take(), Some((1, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    assert_eq!(group1_items_changed.take(), None);
}

#[test]
fn mixed_with_group_at_end() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("singleton 0.0"),
            PlaceholderObject::new("singleton 1.0"),
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
            PlaceholderObject::new("group 1 - item 2.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 3)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    let group1 = items[2].downcast_ref::<GroupingListGroup>().unwrap();

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));

    group1.connect_items_changed(clone!(
            #[strong]
            group1_items_changed,
            move |_, position, removed, added| {
                println!(
                    "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
                );
                assert_eq!(
                    group1_items_changed.replace(Some((position, removed, added))),
                    None
                );
            }
        ));

    // Remove the first item in the group.
    list_store.remove(2);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((0, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Re-add the first item in the group.
    list_store.insert(2, &PlaceholderObject::new("group 1 - item 0.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((0, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Remove the middle item in the group.
    list_store.remove(3);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((1, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Re-add the middle item in the group.
    list_store.insert(3, &PlaceholderObject::new("group 1 - item 1.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((1, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    // Remove the last item in the group.
    list_store.remove(4);

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((2, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );

    // Re-add the last item in the group.
    list_store.insert(4, &PlaceholderObject::new("group 1 - item 2.1"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((2, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.1"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.1"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.1"
    );

    // Remove group and last singleton.
    list_store.splice(1, 4, &[] as &[glib::Object]);

    assert_eq!(model_items_changed.take(), Some((1, 2, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );

    assert_eq!(group1_items_changed.take(), None);
}

#[test]
fn mixed_with_singletons_at_start() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("singleton 0.0"),
            PlaceholderObject::new("singleton 1.0"),
            PlaceholderObject::new("singleton 2.0"),
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 4)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    let group1 = items[3].downcast_ref::<GroupingListGroup>().unwrap();

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));

    group1.connect_items_changed(clone!(
            #[strong]
            group1_items_changed,
            move |_, position, removed, added| {
                println!(
                    "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
                );
                assert_eq!(
                    group1_items_changed.replace(Some((position, removed, added))),
                    None
                );
            }
        ));

    // Remove the first singleton.
    list_store.remove(0);

    assert_eq!(model_items_changed.take(), Some((0, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[2].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);

    // Re-add the first singleton.
    list_store.insert(0, &PlaceholderObject::new("singleton 0.1"));

    assert_eq!(model_items_changed.take(), Some((0, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);

    // Remove the middle singleton.
    list_store.remove(1);

    assert_eq!(model_items_changed.take(), Some((1, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[2].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);

    // Re-add the middle singleton.
    list_store.insert(1, &PlaceholderObject::new("singleton 1.1"));

    assert_eq!(model_items_changed.take(), Some((1, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);

    // Remove the last singleton.
    list_store.remove(2);

    assert_eq!(model_items_changed.take(), Some((2, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);

    // Re-add the last singleton.
    list_store.insert(2, &PlaceholderObject::new("singleton 2.1"));

    assert_eq!(model_items_changed.take(), Some((2, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);
}

#[test]
fn mixed_with_singletons_in_middle() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
            PlaceholderObject::new("singleton 1.0"),
            PlaceholderObject::new("singleton 2.0"),
            PlaceholderObject::new("singleton 3.0"),
            PlaceholderObject::new("group 2 - item 0.0"),
            PlaceholderObject::new("group 2 - item 1.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 5)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    let group1 = items[0].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );
    let group2 = items[4].downcast_ref::<GroupingListGroup>().unwrap();

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );

    let group2_items = group2.snapshot();
    assert_eq!(group2_items.len(), 2);
    assert_eq!(
        group2_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 0.0"
    );
    assert_eq!(
        group2_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 1.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));
    let group2_items_changed = Rc::new(RefCell::new(None));

    group1.connect_items_changed(clone!(
        #[strong]
        group1_items_changed,
        move |_, position, removed, added| {
            println!(
                "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                group1_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));
    group2.connect_items_changed(clone!(
        #[strong]
        group2_items_changed,
        move |_, position, removed, added| {
            println!(
                "group2.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                group2_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    // Remove the first singleton.
    list_store.remove(2);

    assert_eq!(model_items_changed.take(), Some((1, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );
    assert_eq!(
        items[3].downcast_ref::<GroupingListGroup>().unwrap(),
        group2
    );

    assert_eq!(group1_items_changed.take(), None);
    assert_eq!(group2_items_changed.take(), None);

    // Re-add the first singleton.
    list_store.insert(2, &PlaceholderObject::new("singleton 1.1"));

    assert_eq!(model_items_changed.take(), Some((1, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<GroupingListGroup>().unwrap(),
        group2
    );

    assert_eq!(group1_items_changed.take(), None);
    assert_eq!(group2_items_changed.take(), None);

    // Remove the middle singleton.
    list_store.remove(3);

    assert_eq!(model_items_changed.take(), Some((2, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );
    assert_eq!(
        items[3].downcast_ref::<GroupingListGroup>().unwrap(),
        group2
    );

    assert_eq!(group1_items_changed.take(), None);
    assert_eq!(group2_items_changed.take(), None);

    // Re-add the middle singleton.
    list_store.insert(3, &PlaceholderObject::new("singleton 2.1"));

    assert_eq!(model_items_changed.take(), Some((2, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );
    assert_eq!(
        items[4].downcast_ref::<GroupingListGroup>().unwrap(),
        group2
    );

    assert_eq!(group1_items_changed.take(), None);
    assert_eq!(group2_items_changed.take(), None);

    // Remove the last singleton.
    list_store.remove(4);

    assert_eq!(model_items_changed.take(), Some((3, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<GroupingListGroup>().unwrap(),
        group2
    );

    assert_eq!(group1_items_changed.take(), None);
    assert_eq!(group2_items_changed.take(), None);

    // Re-add the last singleton.
    list_store.insert(4, &PlaceholderObject::new("singleton 3.1"));

    assert_eq!(model_items_changed.take(), Some((3, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.1"
    );
    assert_eq!(
        items[4].downcast_ref::<GroupingListGroup>().unwrap(),
        group2
    );

    assert_eq!(group1_items_changed.take(), None);
    assert_eq!(group2_items_changed.take(), None);
}

#[test]
fn mixed_with_singletons_at_end() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
            PlaceholderObject::new("singleton 1.0"),
            PlaceholderObject::new("singleton 2.0"),
            PlaceholderObject::new("singleton 3.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 4)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    let group1 = items[0].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));

    group1.connect_items_changed(clone!(
            #[strong]
            group1_items_changed,
            move |_, position, removed, added| {
                println!(
                    "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
                );
                assert_eq!(
                    group1_items_changed.replace(Some((position, removed, added))),
                    None
                );
            }
        ));

    // Remove the first singleton.
    list_store.remove(2);

    assert_eq!(model_items_changed.take(), Some((1, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group1_items_changed.take(), None);

    // Re-add the first singleton.
    list_store.insert(2, &PlaceholderObject::new("singleton 1.1"));

    assert_eq!(model_items_changed.take(), Some((1, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group1_items_changed.take(), None);

    // Remove the middle singleton.
    list_store.remove(3);

    assert_eq!(model_items_changed.take(), Some((2, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group1_items_changed.take(), None);

    // Re-add the middle singleton.
    list_store.insert(3, &PlaceholderObject::new("singleton 2.1"));

    assert_eq!(model_items_changed.take(), Some((2, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group1_items_changed.take(), None);

    // Remove the last singleton.
    list_store.remove(4);

    assert_eq!(model_items_changed.take(), Some((3, 1, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );

    assert_eq!(group1_items_changed.take(), None);

    // Re-add the last singleton.
    list_store.insert(4, &PlaceholderObject::new("singleton 3.1"));

    assert_eq!(model_items_changed.take(), Some((3, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.1"
    );

    assert_eq!(group1_items_changed.take(), None);
}

#[test]
fn merge_into_group() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(None));

    let model_handler = model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                model_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("singleton 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
            PlaceholderObject::new("singleton 2.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((0, 0, 3)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    // Add one item to group end.
    list_store.insert(2, &PlaceholderObject::new("group 1 - item 4.0"));

    assert_eq!(model_items_changed.take(), Some((1, 1, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    let group1 = items[1].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 4.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));

    group1.connect_items_changed(clone!(
        #[strong]
        group1_items_changed,
        move |_, position, removed, added| {
            println!(
                "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                group1_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    // Add one item to group start.
    list_store.insert(1, &PlaceholderObject::new("group 1 - item 0.0"));

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((0, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 4.0"
    );

    // Add two more items to group.
    list_store.splice(
        3,
        0,
        &[
            PlaceholderObject::new("group 1 - item 2.0"),
            PlaceholderObject::new("group 1 - item 3.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), None);
    assert_eq!(model.n_items(), 3);

    assert_eq!(group1_items_changed.take(), Some((2, 0, 2)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 5);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        group1_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 3.0"
    );
    assert_eq!(
        group1_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 4.0"
    );

    // Add item for group and other singleton at the end.
    list_store.splice(
        7,
        0,
        &[
            PlaceholderObject::new("group 1 - item 5.0"),
            PlaceholderObject::new("singleton 3.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((3, 0, 2)));

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 5.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group1_items_changed.take(), None);

    // Remove second singleton.
    list_store.remove(6);

    assert_eq!(model_items_changed.take(), Some((2, 2, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group1_items_changed.take(), Some((5, 0, 1)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 6);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        group1_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 3.0"
    );
    assert_eq!(
        group1_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 4.0"
    );
    assert_eq!(
        group1_items[5].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 5.0"
    );

    // Add two items for group at the end.
    list_store.splice(
        8,
        0,
        &[
            PlaceholderObject::new("group 1 - item 6.0"),
            PlaceholderObject::new("group 1 - item 7.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((3, 0, 1)));

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );
    let group1_part2 = items[3].downcast_ref::<GroupingListGroup>().unwrap();

    assert_eq!(group1_items_changed.take(), None);

    let group1_part2_items = group1_part2.snapshot();
    assert_eq!(group1_part2_items.len(), 2);
    assert_eq!(
        group1_part2_items[0]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 1 - item 6.0"
    );
    assert_eq!(
        group1_part2_items[1]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 1 - item 7.0"
    );

    let group1_part2_items_changed = Rc::new(RefCell::new(None));

    group1_part2.connect_items_changed(clone!(
        #[strong]
        group1_part2_items_changed,
        move |_, position, removed, added| {
            println!(
                "group1_part2.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(
                group1_part2_items_changed.replace(Some((position, removed, added))),
                None
            );
        }
    ));

    // Remove the last singleton.
    list_store.remove(7);

    assert_eq!(model_items_changed.take(), Some((2, 2, 0)));

    let items = model.snapshot();
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), Some((6, 0, 2)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 8);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        group1_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 3.0"
    );
    assert_eq!(
        group1_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 4.0"
    );
    assert_eq!(
        group1_items[5].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 5.0"
    );
    assert_eq!(
        group1_items[6].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 6.0"
    );
    assert_eq!(
        group1_items[7].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 7.0"
    );

    assert_eq!(group1_part2_items_changed.take(), None);

    // Add two items for another group, separated by a singleton.
    list_store.splice(
        1,
        0,
        &[
            PlaceholderObject::new("group 0 - item 0.0"),
            PlaceholderObject::new("singleton 1.1"),
            PlaceholderObject::new("group 0 - item 1.0"),
            PlaceholderObject::new("singleton 2.1"),
        ],
    );

    assert_eq!(model_items_changed.take(), Some((1, 0, 4)));

    let items = model.snapshot();
    assert_eq!(items.len(), 6);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 0.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 1.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[5].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);

    // We will receive 2 signals for one action.
    model.disconnect(model_handler);
    let model_items_changed = Rc::new(RefCell::new(Vec::new()));
    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            model_items_changed
                .borrow_mut()
                .push((position, removed, added));
        }
    ));

    // Remove the second singleton.
    list_store.remove(2);

    assert_eq!(model_items_changed.take(), &[(2, 2, 0), (1, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    let group0 = items[1].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.1"
    );
    assert_eq!(
        items[3].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );

    assert_eq!(group1_items_changed.take(), None);

    let group0_items = group0.snapshot();
    assert_eq!(group0_items.len(), 2);
    assert_eq!(
        group0_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 0.0"
    );
    assert_eq!(
        group0_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 1.0"
    );
}

#[test]
fn split_group() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(Vec::new()));

    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            model_items_changed
                .borrow_mut()
                .push((position, removed, added));
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), &[]);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("singleton 0.0"),
            PlaceholderObject::new("group 1 - item 2.0"),
            PlaceholderObject::new("group 2 - item 0.0"),
            PlaceholderObject::new("singleton 2.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), &[(0, 0, 3)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    let mixed_group = items[1].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    let mixed_group_items = mixed_group.snapshot();
    assert_eq!(mixed_group_items.len(), 2);
    assert_eq!(
        mixed_group_items[0]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        mixed_group_items[1]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 2 - item 0.0"
    );

    // Add singleton to split group.
    list_store.insert(2, &PlaceholderObject::new("singleton 1.0"));

    assert_eq!(model_items_changed.take(), &[(2, 0, 2), (1, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 0.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    // Add two items to first group.
    list_store.splice(
        1,
        0,
        &[
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), &[(1, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    let group1 = items[1].downcast_ref::<GroupingListGroup>().unwrap();
    assert_ne!(group1, mixed_group);
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 0.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 3);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );

    let group1_items_changed = Rc::new(RefCell::new(None));
    group1.connect_items_changed(clone!(
        #[strong]
        group1_items_changed,
        move |_, position, removed, added| {
            println!(
                "group1.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(group1_items_changed.replace(Some((position, removed, added))),None);
        }
    ));

    // Add singleton to split first group.
    list_store.insert(3, &PlaceholderObject::new("singleton 0.9"));

    assert_eq!(model_items_changed.take(), &[(2, 0, 2)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 7);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.9"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[5].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 0.0"
    );
    assert_eq!(
        items[6].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    assert_eq!(group1_items_changed.take(), Some((2, 1, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );

    // Add two items to second group.
    list_store.splice(
        7,
        0,
        &[
            PlaceholderObject::new("group 2 - item 1.0"),
            PlaceholderObject::new("group 2 - item 2.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), &[(5, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 7);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.9"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    let group2 = items[5].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[6].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    assert_eq!(group1_items_changed.take(), None);

    let group2_items = group2.snapshot();
    assert_eq!(group2_items.len(), 3);
    assert_eq!(
        group2_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 0.0"
    );
    assert_eq!(
        group2_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 1.0"
    );
    assert_eq!(
        group2_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 2.0"
    );

    // Add singleton to split second group.
    list_store.insert(7, &PlaceholderObject::new("singleton 1.9"));

    assert_eq!(model_items_changed.take(), &[(6, 0, 2), (5, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 9);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.9"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[5].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 0.0"
    );
    assert_eq!(
        items[6].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.9"
    );
    let group2_v2 = items[7].downcast_ref::<GroupingListGroup>().unwrap();
    assert_ne!(group2_v2, group2);
    assert_eq!(
        items[8].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    assert_eq!(group1_items_changed.take(), None);

    let group2_v2_items = group2_v2.snapshot();
    assert_eq!(group2_v2_items.len(), 2);
    assert_eq!(
        group2_v2_items[0]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 2 - item 1.0"
    );
    assert_eq!(
        group2_v2_items[1]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 2 - item 2.0"
    );

    // Remove singletons to merge groups.
    list_store.splice(3, 5, &[] as &[glib::Object]);

    assert_eq!(model_items_changed.take(), &[(2, 6, 0)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    assert_eq!(group1_items_changed.take(), Some((2, 0, 2)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 4);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );
    assert_eq!(
        group1_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 1.0"
    );
    assert_eq!(
        group1_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 2.0"
    );

    // Add singleton to re-split group.
    list_store.insert(3, &PlaceholderObject::new("singleton 1.1"));

    assert_eq!(model_items_changed.take(), &[(2, 0, 2)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.1"
    );
    let group2_v3 = items[3].downcast_ref::<GroupingListGroup>().unwrap();
    assert_ne!(group2_v3, group2_v2);
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );

    assert_eq!(group1_items_changed.take(), Some((2, 2, 0)));

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );

    let group2_v3_items = group2_v3.snapshot();
    assert_eq!(group2_v3_items.len(), 2);
    assert_eq!(
        group2_v3_items[0]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 2 - item 1.0"
    );
    assert_eq!(
        group2_v3_items[1]
            .downcast_ref::<PlaceholderObject>()
            .unwrap(),
        "group 2 - item 2.0"
    );
}

#[test]
fn replace() {
    let list_store = gio::ListStore::new::<PlaceholderObject>();
    // Group objects that start with "group".
    let model = GroupingListModel::new(|lhs, rhs| {
        lhs.downcast_ref::<PlaceholderObject>()
            .is_some_and(|obj| obj.id().starts_with("group"))
            && rhs
                .downcast_ref::<PlaceholderObject>()
                .is_some_and(|obj| obj.id().starts_with("group"))
    });

    assert_eq!(model.n_items(), 0);

    let model_items_changed = Rc::new(RefCell::new(Vec::new()));
    model.connect_items_changed(clone!(
        #[strong]
        model_items_changed,
        move |_, position, removed, added| {
            println!(
                "model.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            model_items_changed
                .borrow_mut()
                .push((position, removed, added));
        }
    ));

    model.set_model(Some(list_store.clone()));

    assert_eq!(model_items_changed.take(), &[]);
    assert_eq!(model.n_items(), 0);

    // Add some initial items.
    list_store.splice(
        0,
        0,
        &[
            PlaceholderObject::new("singleton 0.0"),
            PlaceholderObject::new("singleton 1.0"),
            PlaceholderObject::new("group 1 - item 0.0"),
            PlaceholderObject::new("group 1 - item 1.0"),
            PlaceholderObject::new("singleton 2.0"),
            PlaceholderObject::new("singleton 3.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), &[(0, 0, 5)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.0"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    let group1 = items[2].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    let group1_items = group1.snapshot();
    assert_eq!(group1_items.len(), 2);
    assert_eq!(
        group1_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 0.0"
    );
    assert_eq!(
        group1_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 1 - item 1.0"
    );

    // Replace first singleton by other singleton.
    list_store.splice(0, 1, &[PlaceholderObject::new("singleton 0.1")]);

    assert_eq!(model_items_changed.take(), &[(0, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 0.1"
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group1.n_items(), 2);

    // Replace first singleton by group.
    list_store.splice(
        0,
        1,
        &[
            PlaceholderObject::new("group 0 - item 0.0"),
            PlaceholderObject::new("group 0 - item 1.0"),
            PlaceholderObject::new("group 0 - item 2.0"),
            PlaceholderObject::new("group 0 - item 3.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), &[(0, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    let group0 = items[0].downcast_ref::<GroupingListGroup>().unwrap();
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<GroupingListGroup>().unwrap(),
        group1
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    let group0_items = group0.snapshot();
    assert_eq!(group0_items.len(), 4);
    assert_eq!(
        group0_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 0.0"
    );
    assert_eq!(
        group0_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 1.0"
    );
    assert_eq!(
        group0_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 2.0"
    );
    assert_eq!(
        group0_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 3.0"
    );

    assert_eq!(group1.n_items(), 2);

    let group0_items_changed = Rc::new(RefCell::new(None));
    group0.connect_items_changed(clone!(
        #[strong]
        group0_items_changed,
        move |_, position, removed, added| {
            println!(
                "group0.connect_items_changed: position {position}, removed {removed}, added {added}"
            );
            assert_eq!(group0_items_changed.replace(Some((position, removed, added))), None);
        }
    ));

    // Replace second group by other group.
    list_store.splice(
        5,
        2,
        &[
            PlaceholderObject::new("group 2 - item 0.0"),
            PlaceholderObject::new("group 2 - item 1.0"),
        ],
    );

    assert_eq!(model_items_changed.take(), &[(2, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group0
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    let group2 = items[2].downcast_ref::<GroupingListGroup>().unwrap();
    assert_ne!(group1, group2);
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group0_items_changed.take(), None);
    assert_eq!(group0.n_items(), 4);

    let group2_items = group2.snapshot();
    assert_eq!(group2_items.len(), 2);
    assert_eq!(
        group2_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 0.0"
    );
    assert_eq!(
        group2_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 2 - item 1.0"
    );

    // Replace second group by singleton.
    list_store.splice(5, 2, &[PlaceholderObject::new("singleton 1.9")]);

    assert_eq!(model_items_changed.take(), &[(2, 1, 1)]);

    let items = model.snapshot();
    assert_eq!(items.len(), 5);
    assert_eq!(
        items[0].downcast_ref::<GroupingListGroup>().unwrap(),
        group0
    );
    assert_eq!(
        items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.0"
    );
    assert_eq!(
        items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 1.9"
    );
    assert_eq!(
        items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 2.0"
    );
    assert_eq!(
        items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "singleton 3.0"
    );

    assert_eq!(group0_items_changed.take(), None);
    assert_eq!(group0.n_items(), 4);

    // Replace items in group.
    list_store.splice(
        2,
        2,
        &[
            PlaceholderObject::new("group 0 - item 2.1"),
            PlaceholderObject::new("group 0 - item 3.1"),
            PlaceholderObject::new("group 0 - item 4.1"),
        ],
    );

    assert_eq!(model_items_changed.take(), &[]);
    assert_eq!(model.n_items(), 5);

    assert_eq!(group0_items_changed.take(), Some((2, 2, 3)));

    let group0_items = group0.snapshot();
    assert_eq!(group0_items.len(), 5);
    assert_eq!(
        group0_items[0].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 0.0"
    );
    assert_eq!(
        group0_items[1].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 1.0"
    );
    assert_eq!(
        group0_items[2].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 2.1"
    );
    assert_eq!(
        group0_items[3].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 3.1"
    );
    assert_eq!(
        group0_items[4].downcast_ref::<PlaceholderObject>().unwrap(),
        "group 0 - item 4.1"
    );
}
