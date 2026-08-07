//! Fake C source + matching fake disassembly, keyed to the spike's model.

use std::collections::HashMap;

use crate::model::{AsmLine, SourceLine};

pub(crate) fn fake_data() -> (Vec<SourceLine>, HashMap<usize, Vec<AsmLine>>) {
    let mut sources = Vec::new();
    let mut asm = HashMap::new();
    let ops = [
        "mov", "add", "cmp", "lea", "test", "jne", "call", "xor", "shl", "imul",
    ];
    let regs = ["rax", "rbx", "rcx", "rdx", "rdi", "rsi", "r8", "r9"];
    for i in 0..200usize {
        let line_no = (i + 1) as u32;
        let text = match i % 25 {
            7 => format!(
                "    // 中文注释,测试 double-width 列命中: return compute_{}(x)",
                i + 1
            ),
            0 => format!("int compute_{}_fast(int a, int b) {{", i + 1),
            1 => "    long acc = (long)a * b + 0xdeadbeef;".to_string(),
            _ => format!(
                "    acc = acc ^ (a + {:>3}) >> 1;  // line {:>3}",
                (i % 4) + 2,
                i + 1
            ),
        };
        sources.push(SourceLine {
            line_no,
            text: text.into(),
        });

        if (i + 5) % 10 == 0 {
            let n = 3 + (i % 4); // 3..6 asm lines
            let base = 0x1_0000_0000u64 + (i as u64) * 0x80;
            let mut block = Vec::new();
            for a in 0..n {
                let op = &ops[(i + a) % ops.len()];
                let r1 = &regs[(i + a * 2) % regs.len()];
                block.push(AsmLine {
                    addr: base + (a as u64) * 16,
                    text: format!(
                        "    {}   {}, 0x{:x}",
                        op,
                        r1,
                        0x1400 + (i % 240) * 8 + a * 8
                    )
                    .into(),
                });
            }
            asm.insert(i, block);
        }
    }
    (sources, asm)
}
