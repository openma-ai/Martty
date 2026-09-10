# Solution for Issue #43

## 🛠️ Proposed Solution (by Aditya Waghamare)

### Analysis
Martty is a terminal UI application built with Go (bubbletea/lipgloss ecosystem). The text input component currently lacks mouse event handling (like `tea.MouseClick` and `tea.MouseMotion`) to calculate cursor position and text selection based on terminal click coordinates, wide character (width-aware) string slicing, and clipboard integration.

### Fix
Implement mouse event listeners and position calculation helpers inside the input/textarea component, mapping terminal column coordinates to character offsets with proper multi-column (CJK / emoji) width handling, and hook up drag-to-select and click-to-position logic.

### Implementation
```go
// Example patch for input handling in bubbletea component
func (m *Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
    switch msg := msg.(type) {
    case tea.MouseMsg:
        switch msg.Action {
        case tea.MouseActionPress:
            if msg.Button == tea.MouseButtonLeft {
                // Calculate clicked character offset considering multi-width runes
                m.cursor = m.offsetFromCoord(msg.X, msg.Y)
                m.selectionStart = m.cursor
            }
        case tea.MouseActionMotion:
            if msg.Button == tea.MouseButtonLeft {
                // Update selection range on drag
                m.cursor = m.offsetFromCoord(msg.X, msg.Y)
            }
        case tea.MouseActionRelease:
            if msg.Button == tea.MouseButtonLeft && m.selectionStart != m.cursor {
                // Copy selected text to system clipboard
                return m, clipboard.WriteAll(m.SelectedText())
            }
        }
    }
    return m, nil
}
```

### Testing
1. Drag mouse across input text and verify selection highlight appears and text is copied to clipboard.
2. Click at various positions in the input (beginning, middle, trailing whitespace) and verify cursor and subsequent typing align correctly.
3. Test with CJK characters and emojis to ensure correct visual column offset calculation.

Signed-off-by: Aditya Waghamare <adityawaghamare7620@gmail.com>

---
*Submitted by Aditya Waghamare*
💰 **Payout Address (Base L2 / EVM):** `0xb61dBcdBc3407F71EaCb64D4CBFAcf9FFfe2415C`