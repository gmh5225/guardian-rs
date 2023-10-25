#[cfg(test)]
mod tests {
    use obfuscator::vm::machine::{disassemble, Machine};
    use obfuscator::vm::virtualizer::virtualize;

    #[test]
    #[cfg(target_env = "msvc")]
    fn virtualizer_and_machine() {
        const SHELLCODE: &[u8] = &[
            0x89, 0x4c, 0x24, 0x08, 0x8b, 0x44, 0x24, 0x08, 0x0f, 0xaf, 0x44, 0x24, 0x08, 0xc3
        ];
        let m = Machine::new(&virtualize(SHELLCODE)).unwrap();
        let f: extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(m.vmenter.as_ptr::<()>()) };
        assert_eq!(f(2), 4);
    }

    #[test]
    #[cfg(target_env = "msvc")]
    fn virtualize_jmp_lbl() {
        use iced_x86::code_asm::*;
        let mut a = CodeAssembler::new(64).unwrap();
        let mut lbl = a.create_label();

        a.mov(rax, rcx).unwrap(); // move first arg into rax
        a.set_label(&mut lbl).unwrap(); // jmp should land here
        a.sub(rax, 1).unwrap(); // substract 4 from rax
        a.cmp(rax, rdx).unwrap();
        a.jg(lbl).unwrap(); // jmp to label if rax is greater than rdx (loops until rax is rdx)
        a.ret().unwrap(); // return value of rax, should be zero

        let m = Machine::new(&virtualize(&a.assemble(0).unwrap())).unwrap();

        let f: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(m.vmenter.as_ptr::<()>()) };
        assert_eq!(f(21, 0), 0);
        // 0 as second argument i think should not work, bcuz -2 is bigger in actual size than 0
        assert_eq!(f(-2, -1), -3);
    }

    #[test]
    #[cfg(target_env = "msvc")]
    fn virtualize_div() {
        use iced_x86::code_asm::*;
        let mut a = CodeAssembler::new(64).unwrap();
        a.mov(rax, rcx).unwrap(); // mov first argument into rax (dividend)
        a.mov(rcx, rdx).unwrap(); // mov second argument to rcx (divisor)
        a.xor(rdx, rdx).unwrap(); // clear rdx
        a.div(rcx).unwrap(); // 8 / 4 = 2 in rax
        a.ret().unwrap();

        let m = Machine::new(&virtualize(&a.assemble(0).unwrap())).unwrap();
        let f: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(m.vmenter.as_ptr::<()>()) };
        assert_eq!(f(8, 4), 2);
    }

    #[test]
    #[cfg(target_env = "msvc")]
    fn virtualize_push_pop() {
        use iced_x86::code_asm::*;
        let mut a = CodeAssembler::new(64).unwrap();
        a.push(rcx).unwrap();
        a.pop(rax).unwrap();
        a.ret().unwrap();
        let m = Machine::new(&virtualize(&a.assemble(0).unwrap())).unwrap();
        let f: extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(m.vmenter.as_ptr::<()>()) };
        assert_eq!(f(8), 8);
    }
}