use adw::subclass::prelude::*;
use derivative::Derivative;
use glib::{clone, subclass::Signal, WeakRef};
use gtk::{
    gio, glib, prelude::*, CompositeTemplate, ListItem, SignalListItemFactory, SingleSelection,
};
use std::{cell::RefCell, rc::Rc, sync::OnceLock};

use super::{AlbumCell, Library};
use crate::{
    cache::Cache,
    common::{Album, ContentStack, Genre},
};

mod imp {
    use super::*;

    #[derive(Debug, CompositeTemplate, Derivative)]
    #[derivative(Default)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-content-view.ui")]
    pub struct GenreContentView {
        #[template_child]
        pub stack: TemplateChild<ContentStack>,
        #[template_child]
        pub genre_name: TemplateChild<gtk::Label>,
        #[template_child]
        pub album_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub album_grid: TemplateChild<gtk::GridView>,

        #[derivative(Default(value = "gio::ListStore::new::<Album>()"))]
        pub album_list: gio::ListStore,
        pub library: WeakRef<Library>,
        pub cache: RefCell<Option<Rc<Cache>>>,
        pub current_genre: RefCell<Option<Genre>>,
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
            self.stack.show_placeholder();
            self.album_list
                .bind_property("n-items", &self.album_count.get(), "label")
                .transform_to(|_, n: u32| {
                    Some(if n == 1 {
                        "1 album".to_string()
                    } else {
                        format!("{n} albums")
                    })
                })
                .sync_create()
                .build();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![Signal::builder("album-clicked")
                    .param_types([Album::static_type()])
                    .build()]
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

    pub fn bind(&self, genre: &Genre) {
        self.imp().genre_name.set_label(genre.get_name());
        self.imp().current_genre.replace(Some(genre.clone()));
        self.populate();
    }

    pub fn unbind(&self) {
        self.imp().album_list.remove_all();
        self.imp().current_genre.replace(None);
        self.imp().stack.show_placeholder();
    }

    fn populate(&self) {
        self.imp().album_list.remove_all();
        let Some(library) = self.imp().library.upgrade() else {
            return;
        };
        let Some(genre) = self.imp().current_genre.borrow().clone() else {
            return;
        };
        let model = self.imp().album_list.clone();
        let stack = self.imp().stack.get();
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
}
