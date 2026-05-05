use gtk::{glib, prelude::*, subclass::prelude::*, CompositeTemplate};
use glib::{clone, Object};
use std::cell::OnceCell;

use crate::{common::Genre, window::EuphonicaWindow};

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/io/github/htkhiem/Euphonica/gtk/library/genre-tag.ui")]
    pub struct GenreTag {
        #[template_child]
        pub name: TemplateChild<gtk::Label>,
        pub genre: OnceCell<Genre>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GenreTag {
        const NAME: &'static str = "EuphonicaGenreTag";
        type Type = super::GenreTag;
        type ParentType = gtk::Button;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GenreTag {}
    impl WidgetImpl for GenreTag {}
    impl ButtonImpl for GenreTag {}
}

glib::wrapper! {
    pub struct GenreTag(ObjectSubclass<imp::GenreTag>)
        @extends gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Actionable;
}

impl GenreTag {
    pub fn new(genre_name: &str, window: &EuphonicaWindow) -> Self {
        let res: Self = Object::builder().build();
        let genre = Genre::new(genre_name);
        res.imp().name.set_label(genre.get_name());
        let _ = res.imp().genre.set(genre);

        res.connect_clicked(clone!(
            #[weak(rename_to = this)]
            res,
            #[weak]
            window,
            move |_| {
                window.goto_genre(this.imp().genre.get().unwrap());
            }
        ));

        res
    }

    pub fn get_name(&self) -> glib::GString {
        self.imp().name.label()
    }
}
