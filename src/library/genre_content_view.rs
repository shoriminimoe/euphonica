use adw::subclass::prelude::*;
use derivative::Derivative;
use glib::{clone, subclass::Signal, WeakRef};
use gtk::{
    gio, glib, prelude::*, CompositeTemplate, ListItem, SignalListItemFactory, SingleSelection,
};
use std::{cell::{Cell, RefCell}, rc::Rc, sync::OnceLock};

use super::{AlbumCell, ArtistCell, Library};
use crate::{
    cache::Cache,
    common::{Album, Artist, ContentStack, Genre},
};

mod imp {
    use super::*;

    #[derive(Debug, CompositeTemplate, Derivative)]
    #[derivative(Default)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-content-view.ui")]
    pub struct GenreContentView {
        #[template_child]
        pub genre_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub count: TemplateChild<gtk::Label>,
        #[template_child]
        pub albums_btn: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub artists_btn: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub subview_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub albums_stack: TemplateChild<ContentStack>,
        #[template_child]
        pub album_grid: TemplateChild<gtk::GridView>,
        #[template_child]
        pub artists_stack: TemplateChild<ContentStack>,
        #[template_child]
        pub artist_grid: TemplateChild<gtk::GridView>,

        #[derivative(Default(value = "gio::ListStore::new::<Album>()"))]
        pub album_list: gio::ListStore,
        #[derivative(Default(value = "gio::ListStore::new::<Artist>()"))]
        pub artist_list: gio::ListStore,

        pub library: WeakRef<Library>,
        pub cache: RefCell<Option<Rc<Cache>>>,
        pub current_genre: RefCell<Option<Genre>>,

        // Track whether the artists subview has been populated for the current
        // genre. Resets in `unbind()`. Albums are always populated on bind, so
        // there's no equivalent flag for them.
        pub artists_initialized: Cell<bool>,

        // Persists the user's last-selected toggle across `unbind()`/`bind()`
        // within a single session. NOT reset by `unbind()`. Default is true
        // (Albums) on first construction.
        #[derivative(Default(value = "Cell::new(true)"))]
        pub last_subview_albums: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreContentView {
        const NAME: &'static str = "EuphonicaGenreContentView";
        type Type = super::GenreContentView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GenreContentView {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            self.albums_stack.show_placeholder();
            self.artists_stack.show_placeholder();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("album-clicked")
                        .param_types([Album::static_type()])
                        .build(),
                    Signal::builder("artist-clicked")
                        .param_types([Artist::static_type()])
                        .build(),
                ]
            })
        }
    }

    impl WidgetImpl for GenreContentView {}
}

glib::wrapper! {
    pub struct GenreContentView(ObjectSubclass<imp::GenreContentView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for GenreContentView {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl GenreContentView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn setup(&self, library: &Library, cache: Rc<Cache>) {
        self.imp().library.set(Some(library));
        self.imp().cache.replace(Some(cache.clone()));

        self.setup_album_grid(cache.clone());
        self.setup_artist_grid(cache.clone());
        self.setup_toggle();
        self.setup_count_label();
    }

    fn setup_album_grid(&self, cache: Rc<Cache>) {
        let sel_model = SingleSelection::new(Some(self.imp().album_list.clone()));
        self.imp().album_grid.set_model(Some(&sel_model));

        let factory = SignalListItemFactory::new();
        factory.connect_setup(clone!(
            #[weak]
            cache,
            move |_, list_item| {
                let item = list_item
                    .downcast_ref::<ListItem>()
                    .expect("Needs to be ListItem");
                let cell = AlbumCell::new(item, cache, None);
                item.set_child(Some(&cell));
            }
        ));
        factory.connect_bind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let album = item
                .item()
                .and_downcast::<Album>()
                .expect("The item has to be a common::Album.");
            let cell = item
                .child()
                .and_downcast::<AlbumCell>()
                .expect("The child has to be an AlbumCell.");
            cell.bind(&album);
        });
        factory.connect_unbind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = item
                .child()
                .and_downcast::<AlbumCell>()
                .expect("The child has to be an AlbumCell.");
            cell.unbind();
        });
        self.imp().album_grid.set_factory(Some(&factory));

        self.imp().album_grid.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |grid, position| {
                let model = grid.model().expect("The model has to exist.");
                let album = model
                    .item(position)
                    .and_downcast::<Album>()
                    .expect("The item has to be a common::Album.");
                this.emit_by_name::<()>("album-clicked", &[&album.to_value()]);
            }
        ));
    }

    fn setup_artist_grid(&self, cache: Rc<Cache>) {
        let sel_model = SingleSelection::new(Some(self.imp().artist_list.clone()));
        self.imp().artist_grid.set_model(Some(&sel_model));

        let factory = SignalListItemFactory::new();
        factory.connect_setup(clone!(
            #[weak]
            cache,
            move |_, list_item| {
                let item = list_item
                    .downcast_ref::<ListItem>()
                    .expect("Needs to be ListItem");
                // `false`: don't immediately fetch avatars externally — same
                // choice as `ArtistView::setup_gridview` makes.
                let cell = ArtistCell::new(item, cache, false);
                item.set_child(Some(&cell));
            }
        ));
        factory.connect_bind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let artist = item
                .item()
                .and_downcast::<Artist>()
                .expect("The item has to be a common::Artist.");
            let cell = item
                .child()
                .and_downcast::<ArtistCell>()
                .expect("The child has to be an ArtistCell.");
            cell.bind(&artist);
        });
        factory.connect_unbind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = item
                .child()
                .and_downcast::<ArtistCell>()
                .expect("The child has to be an ArtistCell.");
            cell.unbind();
        });
        self.imp().artist_grid.set_factory(Some(&factory));

        self.imp().artist_grid.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |grid, position| {
                let model = grid.model().expect("The model has to exist.");
                let artist = model
                    .item(position)
                    .and_downcast::<Artist>()
                    .expect("The item has to be a common::Artist.");
                this.emit_by_name::<()>("artist-clicked", &[&artist.to_value()]);
            }
        ));
    }

    fn setup_toggle(&self) {
        // Bind toggle button state to stack visible-child-name.
        self.imp()
            .albums_btn
            .bind_property("active", &self.imp().subview_stack.get(), "visible-child-name")
            .transform_to(|_, active: bool| {
                Some(if active { "albums" } else { "artists" })
            })
            .sync_create()
            .build();

        // On every toggle: persist the new state and lazy-populate artists.
        self.imp().albums_btn.connect_toggled(clone!(
            #[weak(rename_to = this)]
            self,
            move |btn| {
                let albums_active = btn.is_active();
                this.imp().last_subview_albums.set(albums_active);
                if !albums_active && !this.imp().artists_initialized.get() {
                    this.populate_artists();
                }
            }
        ));
    }

    fn setup_count_label(&self) {
        // The count label reads from whichever ListStore corresponds to the
        // active subview. Update on either ListStore's n-items change AND on
        // toggle change.
        self.imp().album_list.connect_items_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _, _, _| this.update_count_label()
        ));
        self.imp().artist_list.connect_items_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _, _, _| this.update_count_label()
        ));
        self.imp().albums_btn.connect_toggled(clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.update_count_label()
        ));
    }

    fn update_count_label(&self) {
        let albums_active = self.imp().albums_btn.is_active();
        let n = if albums_active {
            self.imp().album_list.n_items()
        } else {
            self.imp().artist_list.n_items()
        };
        let label = match (albums_active, n) {
            (true, 1) => "1 album".to_string(),
            (true, _) => format!("{n} albums"),
            (false, 1) => "1 artist".to_string(),
            (false, _) => format!("{n} artists"),
        };
        self.imp().count.set_label(&label);
    }

    pub fn bind(&self, genre: &Genre) {
        self.imp().genre_name.set_label(genre.get_name());
        self.imp().current_genre.replace(Some(genre.clone()));

        // Restore the persisted toggle state. set_active() fires the toggled
        // signal only when the state actually changes, so we explicitly call
        // populate_artists() afterward if the persisted state is artists AND
        // we changed nothing (the connect_toggled handler wouldn't have fired).
        let want_albums = self.imp().last_subview_albums.get();
        let was_albums = self.imp().albums_btn.is_active();
        self.imp().albums_btn.set_active(want_albums);

        // Always populate albums on bind (cheap; the album subview is the default).
        self.populate_albums();

        // If the toggle didn't change AND artists is the active subview, the
        // toggled handler didn't fire but we still need artists populated.
        if want_albums == was_albums && !want_albums {
            self.populate_artists();
        }

        self.update_count_label();
    }

    pub fn unbind(&self) {
        self.imp().album_list.remove_all();
        self.imp().artist_list.remove_all();
        self.imp().artists_initialized.set(false);
        self.imp().current_genre.replace(None);
        self.imp().albums_stack.show_placeholder();
        self.imp().artists_stack.show_placeholder();
        // Note: last_subview_albums is intentionally NOT reset — it must
        // persist across binds for in-session toggle persistence.
    }

    fn populate_albums(&self) {
        self.imp().album_list.remove_all();
        let Some(library) = self.imp().library.upgrade() else {
            return;
        };
        let Some(genre) = self.imp().current_genre.borrow().clone() else {
            return;
        };
        let model = self.imp().album_list.clone();
        let stack = self.imp().albums_stack.get();
        stack.show_spinner();
        let name = genre.get_name().to_owned();
        glib::spawn_future_local(async move {
            let _ = library
                .get_albums_by_genre(name, |album| {
                    model.append(&album);
                })
                .await;
            if model.n_items() > 0 {
                stack.show_content();
            } else {
                stack.show_placeholder();
            }
        });
    }

    fn populate_artists(&self) {
        if self.imp().artists_initialized.get() {
            return;
        }
        self.imp().artists_initialized.set(true);
        self.imp().artist_list.remove_all();
        let Some(library) = self.imp().library.upgrade() else {
            return;
        };
        let Some(genre) = self.imp().current_genre.borrow().clone() else {
            return;
        };
        let model = self.imp().artist_list.clone();
        let stack = self.imp().artists_stack.get();
        stack.show_spinner();
        let name = genre.get_name().to_owned();
        glib::spawn_future_local(async move {
            let _ = library
                .get_artists_by_genre(name, |artist| {
                    model.append(&artist);
                })
                .await;
            if model.n_items() > 0 {
                stack.show_content();
            } else {
                stack.show_placeholder();
            }
        });
    }
}
