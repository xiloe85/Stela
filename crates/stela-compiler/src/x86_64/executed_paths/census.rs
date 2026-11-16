//! Why traced blocks execute the host instructions they do: each executed
//! instruction gets one cause from its text (as objdump prints it), the
//! guest state field its `[rdi+disp]` operand names and the kind of guest
//! instruction whose code holds it. The table divides the weighted counts
//! by the guest instructions the blocks retired.
use crate::{GuestAddress, GuestState};
use alloc::{collections::BTreeMap, format, string::String, vec::Vec};
use core::mem::offset_of;

/// What a host instruction is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cause {
    /// The guest instruction's own work: ALU operations, moves between host
    /// registers, constants that are operands.
    Compute,
    /// Branches, side exits and the words an exit hands the engine.
    Control,
    /// An exit stub run out of line, which a linked exit replaces by its
    /// link (counted by the run's `link` region, not here).
    ExitStub,
    /// A guest register loaded from or stored to its home in the state.
    Home,
    /// NZCV formed, stored or read from the state.
    Flags,
    /// The guest memory access itself, and its address arithmetic.
    Access,
    /// The access's range check (lower limit, unreachable address) and
    /// alignment test.
    Check,
    /// FP and SIMD lowering, FPCR and FPSR.
    Simd,
    /// Chain control, paired call frames and self-loop counts.
    Chain,
    /// Pointer authentication.
    Pac,
    /// Constants and guest addresses materialized (ADR, ADRP, MOVZ forms
    /// that need no operation).
    Constant,
}

pub(super) const CAUSES: [Cause; 11] = [
    Cause::Compute,
    Cause::Control,
    Cause::ExitStub,
    Cause::Home,
    Cause::Flags,
    Cause::Access,
    Cause::Check,
    Cause::Simd,
    Cause::Chain,
    Cause::Pac,
    Cause::Constant,
];

impl Cause {
    pub(super) fn name(self) -> &'static str {
        match self {
            Cause::Compute => "compute",
            Cause::Control => "control",
            Cause::ExitStub => "exit-stub",
            Cause::Home => "home",
            Cause::Flags => "flags",
            Cause::Access => "mem-access",
            Cause::Check => "mem-check",
            Cause::Simd => "simd-fp",
            Cause::Chain => "chain",
            Cause::Pac => "pac",
            Cause::Constant => "constant",
        }
    }
}

/// The kind of guest instruction whose code holds a host instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GuestKind {
    Pac,
    Simd,
    Branch,
    Memory,
    Other,
}

pub(super) fn guest_kind(word: u32) -> GuestKind {
    let hint = word & 0xffff_f01f == 0xd503_201f;
    let pac_hint = hint && matches!((word >> 8) & 0xf, 1 | 3);
    let pac_data = word & 0xffff_0000 == 0xdac1_0000 || word & 0xffe0_fc00 == 0x9ac0_3000;
    let pac_branch = word & 0xfe9f_f800 == 0xd61f_0800
        || word & 0xfeff_f800 == 0xd63f_0800
        || word & 0xffff_fbff == 0xd65f_0bff
        || word & 0xffff_fbff == 0xd69f_0bff;
    let pac_load = word & 0xff20_0400 == 0xf820_0400;
    if pac_hint || pac_data || pac_branch || pac_load {
        return GuestKind::Pac;
    }
    let branch = word & 0x7c00_0000 == 0x1400_0000
        || word & 0xff00_0010 == 0x5400_0000
        || word & 0x7e00_0000 == 0x3400_0000
        || word & 0x7e00_0000 == 0x3600_0000
        || word & 0xfe00_0000 == 0xd600_0000;
    if branch {
        return GuestKind::Branch;
    }
    let op0 = (word >> 25) & 0xf;
    if matches!(op0, 0b0111 | 0b1111) {
        return GuestKind::Simd;
    }
    if matches!(op0, 0b0100 | 0b0110 | 0b1100 | 0b1110) {
        return if (word >> 26) & 1 == 1 {
            GuestKind::Simd
        } else {
            GuestKind::Memory
        };
    }
    GuestKind::Other
}

/// The guest state field a displacement from RDI names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Gpr,
    Nzcv,
    Vector,
    FpStatus,
    Check,
    Chain,
    Other,
}

fn field(displacement: usize) -> Field {
    let range = |start: usize, end: usize| (start..end).contains(&displacement);
    if displacement < offset_of!(GuestState, nzcv) {
        Field::Gpr
    } else if range(
        offset_of!(GuestState, nzcv),
        offset_of!(GuestState, exclusive_bytes),
    ) {
        Field::Nzcv
    } else if range(offset_of!(GuestState, v), offset_of!(GuestState, fpcr)) {
        Field::Vector
    } else if range(
        offset_of!(GuestState, fpcr),
        offset_of!(GuestState, exclusive_address),
    ) {
        Field::FpStatus
    } else if range(
        offset_of!(GuestState, unreachable),
        offset_of!(GuestState, pan),
    ) || range(
        offset_of!(GuestState, lower_limit),
        offset_of!(GuestState, loop_retired),
    ) {
        Field::Check
    } else if displacement >= offset_of!(GuestState, anchors)
        && !range(
            offset_of!(GuestState, lower_limit),
            offset_of!(GuestState, loop_retired),
        )
    {
        Field::Chain
    } else {
        Field::Other
    }
}

/// The fields the `[rdi]` and `[rdi+0x..]` operands of `text` name.
fn fields(text: &str) -> Vec<Field> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("[rdi") {
        rest = &rest[at + 4..];
        let displacement = match rest.strip_prefix("+0x") {
            Some(hex) => {
                let digits = hex.find(']').map_or("", |end| &hex[..end]);
                usize::from_str_radix(digits, 16).ok()
            }
            None => rest.starts_with(']').then_some(0),
        };
        if let Some(displacement) = displacement {
            found.push(field(displacement));
        }
    }
    found
}

fn is_mask(operand: &str) -> bool {
    matches!(operand, "0x1" | "0x3" | "0x7" | "0xf")
}

/// The cause of the host instruction `text`, from guest code of `kind`,
/// after `previous`; `out_of_line` past the block's inline code.
pub(super) fn cause(text: &str, kind: GuestKind, previous: &str, out_of_line: bool) -> Cause {
    let (op, operands) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let operands = operands.trim();
    let named = fields(text);
    let has = |wanted: Field| named.contains(&wanted);
    if kind == GuestKind::Pac {
        return Cause::Pac;
    }
    let stub = op == "movabs" && operands.starts_with("rax,")
        || op == "mov" && (operands.starts_with("edx,0x") || operands.starts_with("rax,"))
        || op == "mov" && operands.starts_with("rsp,") && has(Field::Chain)
        || matches!(op, "nop" | "xchg" | "jmp" | "ret");
    if out_of_line && stub {
        return Cause::ExitStub;
    }
    if has(Field::Nzcv)
        || matches!(op, "lahf" | "sahf" | "cmc" | "seto")
        || op == "add" && operands.starts_with("al,0x7f")
    {
        return Cause::Flags;
    }
    let previous_operands = previous
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim());
    let previous_test = previous.starts_with("test")
        && previous_operands
            .rsplit_once(',')
            .is_some_and(|(_, mask)| is_mask(mask));
    let test_mask = op == "test"
        && operands
            .rsplit_once(',')
            .is_some_and(|(_, mask)| is_mask(mask));
    if has(Field::Check)
        || matches!(op, "jae" | "jb") && fields(previous).contains(&Field::Check)
        || kind == GuestKind::Memory && (test_mask || op == "jne" && previous_test)
    {
        return Cause::Check;
    }
    if has(Field::Chain) {
        return Cause::Chain;
    }
    let plain_move = matches!(
        op,
        "mov" | "movdqu" | "movdqa" | "movq" | "movd" | "movups" | "movaps" | "movhps" | "movlps"
    );
    if plain_move && matches!(named.first(), Some(Field::Gpr | Field::Vector)) {
        return Cause::Home;
    }
    let memory = text.contains('[') && named.is_empty() && !text.contains("rsp");
    if has(Field::FpStatus) || kind == GuestKind::Simd && !memory {
        return Cause::Simd;
    }
    if op.starts_with('j') || matches!(op, "ret" | "nop" | "xchg" | "int3") {
        return Cause::Control;
    }
    if op == "movabs" {
        return if kind == GuestKind::Branch {
            Cause::Control
        } else {
            Cause::Constant
        };
    }
    if memory && op != "lea" || op == "lea" && kind == GuestKind::Memory {
        return Cause::Access;
    }
    if kind == GuestKind::Branch {
        return Cause::Control;
    }
    Cause::Compute
}

/// The instructions of `code` as objdump disassembles them, by offset.
pub(super) fn listing(code: &[u8]) -> Vec<(usize, String)> {
    let path = std::env::temp_dir().join(format!(
        "stela-listing-{}-{}.bin",
        std::process::id(),
        code.len()
    ));
    std::fs::write(&path, code).unwrap();
    let text = std::process::Command::new("objdump")
        .args([
            "-D",
            "-b",
            "binary",
            "-m",
            "i386:x86-64",
            "-M",
            "intel",
            "--no-show-raw-insn",
        ])
        .arg(&path)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    text.lines()
        .filter_map(|line| {
            let (offset, rest) = line.trim_start().split_once(":\t")?;
            let text = rest.split_whitespace().collect::<Vec<_>>().join(" ");
            Some((usize::from_str_radix(offset, 16).ok()?, text))
        })
        .collect()
}

/// Weighted counts by cause over the blocks traced so far.
#[derive(Default)]
pub(super) struct Census {
    counts: [u64; CAUSES.len()],
    inline: u64,
    guest: u64,
}

/// One traced block: its code, source and map, where its inline code ends
/// and how often each host instruction ran.
pub(super) struct Traced<'a> {
    pub weight: u64,
    pub guest: u64,
    pub pc: u64,
    pub source: &'a [u8],
    pub code: &'a [u8],
    pub map: &'a [(GuestAddress, usize)],
    pub inline_end: usize,
    pub runs: &'a BTreeMap<u32, u32>,
}

impl Census {
    pub(super) fn add(&mut self, block: &Traced) {
        if block.guest == 0 || block.map.is_empty() {
            return;
        }
        self.guest += block.weight * block.guest;
        let mut previous = String::new();
        for (offset, text) in listing(block.code) {
            let ran = u64::from(block.runs.get(&(offset as u32)).copied().unwrap_or(0));
            if ran != 0 {
                let span = block
                    .map
                    .iter()
                    .rposition(|&(_, start)| start <= offset)
                    .unwrap_or(0);
                let at = (block.map[span].0.0 - block.pc) as usize;
                let word = u32::from_le_bytes(block.source[at..at + 4].try_into().unwrap());
                let out_of_line = offset >= block.inline_end;
                let found = cause(&text, guest_kind(word), &previous, out_of_line);
                let index = CAUSES.iter().position(|&c| c == found).unwrap();
                self.counts[index] += block.weight * ran;
                if !out_of_line {
                    self.inline += block.weight * ran;
                }
            }
            previous = text;
        }
    }

    /// One line per cause: share of the executed host instructions and host
    /// instructions per guest instruction, then the totals.
    pub(super) fn table(&self) -> String {
        let total: u64 = self.counts.iter().sum();
        let guest = self.guest.max(1) as f64;
        let mut order: Vec<usize> = (0..CAUSES.len()).collect();
        order.sort_by_key(|&index| core::cmp::Reverse(self.counts[index]));
        let mut text = format!(
            "T total host_per_guest={:.3} inline={:.3} guest={}\n",
            total as f64 / guest,
            self.inline as f64 / guest,
            self.guest
        );
        for index in order {
            text += &format!(
                "T {} share={:.1}% host_per_guest={:.3}\n",
                CAUSES[index].name(),
                100.0 * self.counts[index] as f64 / total.max(1) as f64,
                self.counts[index] as f64 / guest
            );
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_instructions_take_the_cause_their_operands_and_guest_kind_name() {
        let ldr = guest_kind(0xb940_16e8);
        let cmp = guest_kind(0x6b0a_013f);
        let b = guest_kind(0x17ff_bffe);
        let fadd = guest_kind(0x1e22_2800);
        let pacibsp = guest_kind(0xd503_237f);
        assert_eq!(ldr, GuestKind::Memory);
        assert_eq!(cmp, GuestKind::Other);
        assert_eq!(b, GuestKind::Branch);
        assert_eq!(fadd, GuestKind::Simd);
        assert_eq!(pacibsp, GuestKind::Pac);
        assert_eq!(guest_kind(0xdac1_0230), GuestKind::Pac);
        assert_eq!(guest_kind(0xd65f_0bff), GuestKind::Pac);
        let x23 = offset_of!(GuestState, x) + 23 * 8;
        let nzcv = offset_of!(GuestState, nzcv);
        let limit = offset_of!(GuestState, lower_limit);
        let anchors = offset_of!(GuestState, anchors);
        let cases = [
            (
                format!("mov rax,QWORD PTR [rdi+{x23:#x}]"),
                ldr,
                "",
                false,
                Cause::Home,
            ),
            (
                format!("cmp rax,QWORD PTR [rdi+{limit:#x}]"),
                ldr,
                "",
                false,
                Cause::Check,
            ),
            (
                "jae 0x6e".into(),
                ldr,
                "cmp rax,QWORD PTR [rdi+0x378]",
                false,
                Cause::Check,
            ),
            ("test bl,0xf".into(), ldr, "", false, Cause::Check),
            ("jne 0x12a".into(), ldr, "test bl,0xf", false, Cause::Check),
            (
                "mov esi,DWORD PTR [rax+0x14]".into(),
                ldr,
                "",
                false,
                Cause::Access,
            ),
            ("lea r15,[r15+0x8]".into(), ldr, "", false, Cause::Access),
            ("lahf".into(), cmp, "", false, Cause::Flags),
            (
                format!("mov WORD PTR [rdi+{nzcv:#x}],ax"),
                cmp,
                "",
                false,
                Cause::Flags,
            ),
            ("sub esi,ecx".into(), cmp, "", false, Cause::Compute),
            ("jb 0x85".into(), b, "", false, Cause::Control),
            (
                "movabs rax,0x188000800".into(),
                b,
                "",
                true,
                Cause::ExitStub,
            ),
            (
                "movabs rax,0x188000800".into(),
                b,
                "",
                false,
                Cause::Control,
            ),
            (
                "movabs rbp,0x1f53147a0".into(),
                cmp,
                "",
                false,
                Cause::Constant,
            ),
            (
                format!("mov rcx,QWORD PTR [rdi+{anchors:#x}]"),
                b,
                "",
                false,
                Cause::Chain,
            ),
            ("addss xmm0,xmm1".into(), fadd, "", false, Cause::Simd),
            (
                "movdqa xmm0,XMMWORD PTR [rax]".into(),
                fadd,
                "",
                false,
                Cause::Access,
            ),
            ("rol rcx,0x20".into(), pacibsp, "", false, Cause::Pac),
        ];
        for (text, kind, previous, out_of_line, expected) in cases {
            assert_eq!(
                cause(&text, kind, previous, out_of_line),
                expected,
                "{text}"
            );
        }
    }
}
