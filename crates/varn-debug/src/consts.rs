use varn_utilities::chalk::chalk;
use varn_utilities::terminal::Section;

pub fn debug_consts(filename: &str) {
    Section::new("constant folding").subtitle(filename).color(|c| c.yellow()).print();
    varn_utilities::terminal::log(format!("  {}", chalk("(Constant evaluation trace not implemented)").dim()));
    Section::new("constant folding").close();
}
