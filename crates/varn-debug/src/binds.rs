use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_utilities::terminal::Section;

pub fn debug_binds(filename: &str) {
    Section::new("identifier binding").subtitle(filename).color(|c| c.blue()).print();
    terminal::log(format!("  {}", chalk("(Binding trace not implemented)").dim()));
    Section::new("identifier binding").close();
}
