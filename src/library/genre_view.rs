use adw::prelude::*;
use adw::subclass::prelude::*;
use glib::{clone, subclass::Signal, Properties, WeakRef};
use gtk::{
    glib, CompositeTemplate, ListItem, SignalListItemFactory, SingleSelection,
};
use std::{cell::Cell, cmp::Ordering, rc::Rc, sync::OnceLock};

use super::{GenreCell, GenreContentView, Library};
use crate::{
    cache::Cache,
    common::{ContentStack, Genre},
    utils::{g_cmp_str_options, g_search_substr, settings_manager, LazyInit},
};

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate, Properties)]
    #[properties(wrapper_type = super::GenreView)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-view.ui")]
    pub struct GenreView {
        #[template_child]
        pub nav_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub show_sidebar: TemplateChild<gtk::Button>,

        #[template_child]
        pub sort_dir: TemplateChild<gtk::Image>,
        #[template_child]
        pub sort_dir_btn: TemplateChild<gtk::Button>,
        #[template_child]
        pub search_btn: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,

        #[template_child]
        pub stack: TemplateChild<ContentStack>,
        #[template_child]
        pub grid_view: TemplateChild<gtk::GridView>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_view: TemplateChild<GenreContentView>,

        pub search_filter: gtk::CustomFilter,
        pub sorter: gtk::CustomSorter,
        pub last_search_len: Cell<usize>,
        #[property(get, set)]
        pub collapsed: Cell<bool>,
        pub library: WeakRef<Library>,
        pub initializing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreView {
        const NAME: &'static str = "EuphonicaGenreView";
        type Type = super::GenreView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for GenreView {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }

        fn constructed(&self) {
            self.parent_constructed();
            self.stack.show_placeholder();

            self.obj()
                .bind_property("collapsed", &self.show_sidebar.get(), "visible")
                .sync_create()
                .build();
            self.show_sidebar.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    this.obj().emit_by_name::<()>("show-sidebar-clicked", &[]);
                }
            ));
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("show-sidebar-clicked").build()])
        }
    }

    impl WidgetImpl for GenreView {}
}

glib::wrapper! {
    pub struct GenreView(ObjectSubclass<imp::GenreView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for GenreView {
    fn default() -> Self {
        Self::new()
    }
}

impl GenreView {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn get_content_view(&self) -> GenreContentView {
        self.imp().content_view.get()
    }

    pub fn setup(&self, library: &Library, cache: Rc<Cache>) {
        self.imp().library.set(Some(library));
        self.setup_sort();
        self.setup_search();
        self.setup_gridview(cache.clone());
        self.imp().content_view.get().setup(library, cache);
    }

    fn setup_sort(&self) {
        let settings = settings_manager();
        let state = settings.child("state").child("genreview");
        let library_settings = settings.child("library");
        let sort_dir_btn = self.imp().sort_dir_btn.get();
        sort_dir_btn.connect_clicked(clone!(
            #[weak]
            state,
            move |_| {
                if state.string("sort-direction") == "asc" {
                    let _ = state.set_string("sort-direction", "desc");
                } else {
                    let _ = state.set_string("sort-direction", "asc");
                }
            }
        ));
        let sort_dir = self.imp().sort_dir.get();
        state
            .bind("sort-direction", &sort_dir, "icon-name")
            .get_only()
            .mapping(|dir, _| match dir.get::<String>().unwrap().as_ref() {
                "asc" => Some("view-sort-ascending-symbolic".to_value()),
                _ => Some("view-sort-descending-symbolic".to_value()),
            })
            .build();

        self.imp().sorter.set_sort_func(clone!(
            #[strong]
            library_settings,
            #[strong]
            state,
            move |a, b| {
                let g1 = a.downcast_ref::<Genre>().expect("Sort obj must be Genre.");
                let g2 = b.downcast_ref::<Genre>().expect("Sort obj must be Genre.");
                let asc = state.enum_("sort-direction") > 0;
                let case_sensitive = library_settings.boolean("sort-case-sensitive");
                let nulls_first = library_settings.boolean("sort-nulls-first");
                g_cmp_str_options(
                    Some(g1.get_name()),
                    Some(g2.get_name()),
                    nulls_first,
                    asc,
                    case_sensitive,
                )
            }
        ));

        state.connect_changed(
            Some("sort-direction"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.imp().sorter.changed(gtk::SorterChange::Inverted);
                }
            ),
        );
    }

    fn setup_search(&self) {
        let settings = settings_manager();
        let library_settings = settings.child("library");
        self.imp().search_filter.set_filter_func(clone!(
            #[weak(rename_to = this)]
            self,
            #[strong]
            library_settings,
            #[upgrade_or]
            true,
            move |obj| {
                let genre = obj
                    .downcast_ref::<Genre>()
                    .expect("Search obj must be Genre.");
                let term = this.imp().search_entry.text();
                if term.is_empty() {
                    return true;
                }
                let case_sensitive = library_settings.boolean("search-case-sensitive");
                g_search_substr(Some(genre.get_name()), &term, case_sensitive)
            }
        ));
        let search_entry = self.imp().search_entry.get();
        search_entry.connect_search_changed(clone!(
            #[weak(rename_to = this)]
            self,
            move |entry| {
                let new_len = entry.text().len();
                let old_len = this.imp().last_search_len.replace(new_len);
                match new_len.cmp(&old_len) {
                    Ordering::Greater => this
                        .imp()
                        .search_filter
                        .changed(gtk::FilterChange::MoreStrict),
                    Ordering::Less => this
                        .imp()
                        .search_filter
                        .changed(gtk::FilterChange::LessStrict),
                    Ordering::Equal => this
                        .imp()
                        .search_filter
                        .changed(gtk::FilterChange::Different),
                }
            }
        ));
    }

    fn setup_gridview(&self, _cache: Rc<Cache>) {
        let settings = settings_manager().child("ui");
        let library = self.imp().library.upgrade().unwrap();
        let genres = library.genres();

        let search_bar = self.imp().search_bar.get();
        let search_entry = self.imp().search_entry.get();
        search_bar.connect_entry(&search_entry);
        let search_btn = self.imp().search_btn.get();
        search_btn
            .bind_property("active", &search_bar, "search-mode-enabled")
            .sync_create()
            .build();

        let search_model = gtk::FilterListModel::new(
            Some(genres.clone()),
            Some(self.imp().search_filter.clone()),
        );
        search_model.set_incremental(true);
        let sort_model =
            gtk::SortListModel::new(Some(search_model), Some(self.imp().sorter.clone()));
        sort_model.set_incremental(true);
        let sel_model = SingleSelection::new(Some(sort_model));

        let grid_view = self.imp().grid_view.get();
        grid_view.set_model(Some(&sel_model));
        settings
            .bind("max-columns", &grid_view, "max-columns")
            .build();

        let factory = SignalListItemFactory::new();
        factory.connect_setup(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = GenreCell::new();
            item.set_child(Some(&cell));
        });
        factory.connect_bind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let genre = item
                .item()
                .and_downcast::<Genre>()
                .expect("The item has to be a common::Genre.");
            let cell = item
                .child()
                .and_downcast::<GenreCell>()
                .expect("The child has to be a GenreCell.");
            cell.bind(&genre);
        });
        factory.connect_unbind(move |_, list_item| {
            let item = list_item
                .downcast_ref::<ListItem>()
                .expect("Needs to be ListItem");
            let cell = item
                .child()
                .and_downcast::<GenreCell>()
                .expect("The child has to be a GenreCell.");
            cell.unbind();
        });
        grid_view.set_factory(Some(&factory));

        grid_view.connect_activate(clone!(
            #[weak(rename_to = this)]
            self,
            move |grid, position| {
                let model = grid.model().expect("The model has to exist.");
                let genre = model
                    .item(position)
                    .and_downcast::<Genre>()
                    .expect("The item has to be a common::Genre.");
                this.on_genre_clicked(&genre);
            }
        ));
    }

    pub fn on_genre_clicked(&self, genre: &Genre) {
        let content_view = self.imp().content_view.get();
        content_view.unbind();
        content_view.bind(genre);
        if self
            .imp()
            .nav_view
            .visible_page_tag()
            .is_none_or(|tag| tag.as_str() != "content")
        {
            self.imp().nav_view.push_by_tag("content");
        }
    }
}

impl LazyInit for GenreView {
    fn populate(&self) {
        if let Some(library) = self.imp().library.upgrade() {
            if !self.imp().initializing.get() {
                self.imp().initializing.set(true);
                let stack = self.imp().stack.get();
                let this = self.clone();
                stack.show_spinner();
                glib::spawn_future_local(async move {
                    let _ = library.init_genres().await;
                    if library.genres().n_items() > 0 {
                        stack.show_content();
                    } else {
                        stack.show_placeholder();
                    }
                    this.imp().initializing.set(false);
                });
            }
        }
    }
}
