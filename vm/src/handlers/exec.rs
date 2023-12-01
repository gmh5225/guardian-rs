use vm_proc::handler;
use crate::{Machine, reloc_instr};
use crate::shared::OpSize;

#[handler]
pub unsafe fn vm_exec(vm: &mut Machine, _op_size: OpSize) {
    let instr_size = vm.pc.read_unaligned() as usize;
    vm.pc = vm.pc.add(1); // skip instr size
    reloc_instr(vm, vm.pc, instr_size);

    vm.pc = vm.pc.add(instr_size);
}