pub struct Sample<'a, const LABELS: usize> {
    label_values: [&'a str; LABELS],
    value: f32,
}

pub type LabelValueIter<'a, const LABELS: usize> = core::array::IntoIter<&'a str, LABELS>;

impl<'a, const LABELS: usize> Sample<'a, LABELS> {
    pub const fn new(label_values: [&'a str; LABELS], value: f32) -> Self {
        Self {
            label_values,
            value,
        }
    }

    pub fn set(&mut self, value: f32) {
        self.value = value;
    }

    pub fn incr(&mut self, value: f32) {
        self.value += value;
    }

    pub fn get(&self) -> f32 {
        self.value
    }

    pub fn get_label_values(&self) -> core::array::IntoIter<&'a str, LABELS>  {
        let f: core::array::IntoIter<&'a str, LABELS> = self.label_values.into_iter();

        f
    }
}
