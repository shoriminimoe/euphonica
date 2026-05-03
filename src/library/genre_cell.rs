use gtk::{glib, CompositeTemplate, prelude::*, subclass::prelude::*};

use crate::common::Genre;

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-cell.ui")]
    pub struct GenreCell {
        #[template_child]
        pub name: TemplateChild<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreCell {
        const NAME: &'static str = "EuphonicaGenreCell";
        type Type = super::GenreCell;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GenreCell {}
    impl WidgetImpl for GenreCell {}
    impl BoxImpl for GenreCell {}
}

glib::wrapper! {
    pub struct GenreCell(ObjectSubclass<imp::GenreCell>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl Default for GenreCell {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl GenreCell {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&self, genre: &Genre) {
        self.imp().name.set_label(genre.get_name());
    }

    pub fn unbind(&self) {
        self.imp().name.set_label("");
    }
}
