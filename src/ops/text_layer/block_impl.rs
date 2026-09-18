impl TextBlock {
    /// Get the total flat text across all runs.
    pub fn flat_text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Total character count across all runs.
    pub fn char_count(&self) -> usize {
        self.runs.iter().map(|r| r.text.chars().count()).sum()
    }

    /// Convert a flat byte offset (into the concatenated text) to a `RunPosition`.
    pub fn flat_offset_to_run_pos(&self, flat_byte: usize) -> RunPosition {
        let mut remaining = flat_byte;
        for (ri, run) in self.runs.iter().enumerate() {
            let len = run.text.len();
            if remaining <= len {
                return RunPosition {
                    run_index: ri,
                    byte_offset: remaining,
                };
            }
            remaining -= len;
        }
        // Past end — clamp to end of last run
        RunPosition {
            run_index: self.runs.len().saturating_sub(1),
            byte_offset: self.runs.last().map_or(0, |r| r.text.len()),
        }
    }

    /// Convert a `RunPosition` to a flat byte offset.
    pub fn run_pos_to_flat_offset(&self, pos: RunPosition) -> usize {
        let mut offset = 0;
        for (ri, run) in self.runs.iter().enumerate() {
            if ri == pos.run_index {
                return offset + pos.byte_offset.min(run.text.len());
            }
            offset += run.text.len();
        }
        offset
    }

    /// Apply a `TextStyle` to the range `[start_byte..end_byte)` (flat byte offsets).
    /// Splits runs at boundaries as needed and applies the style to the middle portion.
    /// Returns the new run positions for the affected range.
    pub fn apply_style_to_range(
        &mut self,
        start_byte: usize,
        end_byte: usize,
        apply: impl Fn(&mut TextStyle),
    ) {
        if start_byte >= end_byte {
            return;
        }

        // Strategy: split runs at start and end boundaries, then apply style to
        // all runs fully inside the range.
        self.split_at_flat_offset(end_byte);
        self.split_at_flat_offset(start_byte);

        // Now apply the style to runs within [start_byte .. end_byte)
        let mut offset = 0usize;
        for run in &mut self.runs {
            let run_end = offset + run.text.len();
            if offset >= start_byte && run_end <= end_byte && !run.text.is_empty() {
                apply(&mut run.style);
            }
            offset = run_end;
        }

        self.merge_adjacent_runs();
    }

    /// Split runs so that there is a run boundary at the given flat byte offset.
    fn split_at_flat_offset(&mut self, flat_byte: usize) {
        let mut offset = 0usize;
        for i in 0..self.runs.len() {
            let run_len = self.runs[i].text.len();
            if offset == flat_byte || offset + run_len <= flat_byte {
                offset += run_len;
                continue;
            }
            // flat_byte falls inside this run — split it
            let local = flat_byte - offset;
            if local > 0 && local < run_len {
                let tail_text = self.runs[i].text[local..].to_string();
                let tail_style = self.runs[i].style.clone();
                self.runs[i].text.truncate(local);
                self.runs.insert(
                    i + 1,
                    TextRun {
                        text: tail_text,
                        style: tail_style,
                    },
                );
            }
            return;
        }
    }

    /// Merge adjacent runs that have identical styles.
    pub fn merge_adjacent_runs(&mut self) {
        let mut i = 0;
        while i + 1 < self.runs.len() {
            if self.runs[i].style == self.runs[i + 1].style {
                let next_text = self.runs[i + 1].text.clone();
                self.runs[i].text.push_str(&next_text);
                self.runs.remove(i + 1);
            } else if self.runs[i].text.is_empty() {
                self.runs.remove(i);
            } else {
                i += 1;
            }
        }
        // Remove trailing empty runs (keep at least one)
        while self.runs.len() > 1 && self.runs.last().is_some_and(|r| r.text.is_empty()) {
            self.runs.pop();
        }
    }

    /// Insert text at a flat byte offset, inheriting the style of the run at that position.
    pub fn insert_text_at(&mut self, flat_byte: usize, text: &str) {
        let pos = self.flat_offset_to_run_pos(flat_byte);
        if pos.run_index < self.runs.len() {
            self.runs[pos.run_index]
                .text
                .insert_str(pos.byte_offset, text);
        } else if let Some(last) = self.runs.last_mut() {
            last.text.push_str(text);
        }
    }

    /// Delete text in the range `[start_byte..end_byte)` (flat byte offsets).
    pub fn delete_range(&mut self, start_byte: usize, end_byte: usize) {
        if start_byte >= end_byte {
            return;
        }
        // Walk runs and delete the overlapping portion from each
        let mut offset = 0usize;
        let mut i = 0;
        while i < self.runs.len() {
            let run_len = self.runs[i].text.len();
            let run_start = offset;
            let run_end = offset + run_len;

            if run_end <= start_byte || run_start >= end_byte {
                // No overlap
                offset = run_end;
                i += 1;
                continue;
            }

            let del_start = start_byte.max(run_start) - run_start;
            let del_end = end_byte.min(run_end) - run_start;

            // Remove the range from this run's text
            let before = &self.runs[i].text[..del_start];
            let after = &self.runs[i].text[del_end..];
            self.runs[i].text = format!("{}{}", before, after);

            offset = run_start + self.runs[i].text.len();
            i += 1;
        }
        self.merge_adjacent_runs();
    }

    /// Find the override for a specific glyph index, if any.
    pub fn get_glyph_override(&self, glyph_index: usize) -> Option<&GlyphOverride> {
        self.glyph_overrides
            .iter()
            .find(|o| o.glyph_index == glyph_index)
    }

    /// Get or insert a mutable override for a glyph index.
    pub fn ensure_glyph_override(&mut self, glyph_index: usize) -> &mut GlyphOverride {
        if let Some(pos) = self
            .glyph_overrides
            .iter()
            .position(|o| o.glyph_index == glyph_index)
        {
            &mut self.glyph_overrides[pos]
        } else {
            self.glyph_overrides.push(GlyphOverride {
                glyph_index,
                ..Default::default()
            });
            self.glyph_overrides.last_mut().unwrap()
        }
    }

    /// Remove identity overrides (cleanup after editing).
    pub fn cleanup_glyph_overrides(&mut self) {
        self.glyph_overrides.retain(|o| !o.is_identity());
    }

    /// Clear all glyph overrides (reset to default positioning).
    pub fn clear_glyph_overrides(&mut self) {
        self.glyph_overrides.clear();
    }

    /// Returns true if any glyph overrides are present.
    pub fn has_glyph_overrides(&self) -> bool {
        !self.glyph_overrides.is_empty()
    }
}

