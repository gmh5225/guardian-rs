mod diassembler;
mod pe;
mod vm;

use std::fs;
use std::fs::File;
use std::mem::size_of_val;

use crate::diassembler::Disassembler;
use crate::vm::machine::{Assembler, disassemble, Machine, Register};
use crate::vm::virtualizer::{virtualize, virtualize_with_ip};

// virtualization of code that is in between a call of function like begin_virtualization and end_virtualization
// which are imported from a stub dll, the code is virtualized, a machine is created from the virtual code and the
// original code segment is replaced by the vmentry of the machine


// todo
// i need to somehow embed the machine.rs as a new section
// OR run machine and embed the run function as a new section
// which is probably easier
// so then i create a machine from code i disassemble in the binary
// so the machine instead of allocating vmenter and exit in the heap
// needs to do so by creating it ahead of time in a section for
// every virtualized function/code block
// then it might be easier to just compile the machine
// and create it dynamically by using something like this
// lea arg1, bytecode
// mov rax, machine::new
// call rax // create new machine
// lea rax, ret_val.vm_enter
// call rax // executes vm
// if not then see below:
// and replace the code with something like (pseudo asm)
// mov arg1, machineforthiscode (section .machines offset 0x6900)
// lea rax, virtual machine (run function, section .vm offset 0)
// call rax (call run function with ptr to machine struct)

use exe::{Buffer, CCharString, Error, ImageSectionHeader, NTHeadersMut, Offset, PE, PEType, RelocationDirectory, RVA, SectionCharacteristics, VecPE};
use iced_x86::code_asm::{AsmRegister64, CodeAssembler};
use iced_x86::Decoder;
use memoffset::offset_of;
use symbolic_demangle::Demangle;

fn main() {
    let mut a = Assembler::default();
    let x = 8u64;
    let y = 8u64;
    let mut z = 0u64;

    a.const_(&x as *const _ as u64);
    a.load();
    a.const_(&y as *const _ as u64);
    a.load();
    a.cmp();
    a.add();
    a.const_(&mut z as *mut _ as u64);
    a.store();

    unsafe { Machine::new(&a.assemble()).unwrap().run() };
    assert_eq!(z, 16);
    #[repr(C)]
    pub struct TestMachine {
        pub(crate) pc: *const u8,
        pub(crate) sp: *mut u64,
        pub regs: [u64; 16],
        pub rflags: u64,
        pub(crate) program: *const u8,
        pub(crate) vmstack: Vec<u64>,
        pub(crate) cpustack: Vec<u8>,
        pub cpustack_ptr: *const u8,
        vmexit: *const u64,
    }
    let test = 0x1000 - 0x100 - std::mem::size_of::<u64>();
    println!("test: {:x}", test);
    // "../reddeadonline/target/x86_64-pc-windows-msvc/release-lto/loader.exe"
    let map_data = std::fs::read("../hello_world/target/release/hello_world.map").unwrap();
    let map_string = String::from_utf8(map_data).unwrap();
    let mut map_file = pe::parser::MapFile::load(&map_string).unwrap();
    println!("{}", map_file.functions.len());

    let (function, function_size) = map_file.get_function("hello_world::calc").unwrap();
    println!("target function: {}: {:x}", function.symbol, function_size);

    let mut pefile = VecPE::from_disk_file("../hello_world/target/release/hello_world.exe").unwrap();

    // relocating will probably be done dynamically
    // have to mark them as relocate somehow
    // but for jmps i need to be able to identify label with target
    let data: Vec<u8> = virtualize_with_ip(0000000180000000, &[  0x89, 0x4c, 0x24, 0x08, 0x8b, 0x44, 0x24, 0x08, 0x0f, 0xaf, 0x44, 0x24, 0x08, 0xc2, 0x00,
        0x00,]);
    println!("{:?}", &data);

    let m = Machine::new(&data).unwrap();
    let f: extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(m.vmenter.as_ptr::<()>()) };
    assert_eq!(f(2), 4);
    println!("{}", f(6));
    let mut bytecode_section = ImageSectionHeader::default();
    bytecode_section.set_name(Some(".byte"));
    bytecode_section.virtual_size = 0x1000;
    bytecode_section.size_of_raw_data = data.len() as u32;
    bytecode_section.characteristics = SectionCharacteristics::MEM_EXECUTE
        | SectionCharacteristics::MEM_READ
        | SectionCharacteristics::CNT_CODE;

    let bytecode_section = add_section(&mut pefile, &bytecode_section, &data).unwrap();

    // todo place all the bytecode into the bytecode section for every virtualized code part
    // in this case it would just be data


    let mut vm_file = VecPE::from_disk_file("target/x86_64-pc-windows-msvc/release/vm.dll").unwrap();
    let vm_file_text = vm_file.get_section_by_name(".text").unwrap().clone();
    let machine_entry = vm_file.get_entrypoint().unwrap();
    println!("vm machine::new: {:x}", machine_entry.0);

    let mut machine = vm_file.read(vm_file_text.data_offset(pefile.get_type()), vm_file_text.size_of_raw_data as _)
        .unwrap();

    let mut vm1 = create_machine(bytecode_section.virtual_address.0 as _, data.len());

    let mut vm_section = ImageSectionHeader::default();
    vm_section.set_name(Some(".vm"));
    vm_section.virtual_size = (machine.len() + 0x1000) as u32; // 30kb, vm is 26kb
    vm_section.size_of_raw_data = machine.len() as u32 + size_of_val(&vm1) as u32;
    vm_section.characteristics = SectionCharacteristics::MEM_EXECUTE
        | SectionCharacteristics::MEM_READ
        | SectionCharacteristics::CNT_CODE;

    // todo include compiled machine into vm section (look independent shellcode maybe as reference)
    let vm_section = add_section(&mut pefile, &vm_section, &machine.to_vec()).unwrap();

    let machine_addr = vm_section.virtual_address.0 + vm_section.size_of_raw_data - size_of_val(&vm1) as u32;

    // test patching
    let target_fn_addr = pefile.rva_to_offset(RVA(function.rva.0 as _)).unwrap().0 as _;
    let target_function = pefile.get_slice_ref::<u8>(target_fn_addr, function_size).expect("rawr");
    let function_size = Disassembler::from_bytes(target_function.to_vec()).disassemble();
    println!("real size: {:x}", function_size);
    let patch_len = patch_function(&mut pefile, target_fn_addr, function_size, vm_section.virtual_address.0 + machine_entry.0 - 0x1000, bytecode_section.virtual_address.0);
    let target_function_mut = pefile.get_slice_ref::<u8>(target_fn_addr, patch_len).expect("rawr");
    Disassembler::from_bytes(target_function_mut.to_vec()).disassemble();
    //

    // create machine linking to program at start of bytecode section
    let nt_headers = pefile.get_valid_mut_nt_headers();
    assert!(nt_headers.is_ok());
    if let NTHeadersMut::NTHeaders64(nt_headers_64) = nt_headers.unwrap() {
        generate_vm_entry(
            &mut vm1,
           machine_addr as u64,
            nt_headers_64.optional_header.image_base as usize + (vm_section.virtual_address.0 + machine_entry.0 - 0x1000) as usize);
    }
    println!("{:?}", vm1.vmenter.clone().to_vec());
    Disassembler::from_bytes(vm1.vmenter.clone().to_vec()).disassemble();
    generate_vm_exit(&mut vm1);
    println!("{:?}", vm1.vmexit.clone().to_vec());
    println!("--------------------------------");
    Disassembler::from_bytes(vm1.vmexit.clone().to_vec()).disassemble();
    let mut buffer = vec![0u8; size_of_val(&vm1)];
    unsafe { std::ptr::copy(&vm1 as *const _ as *const u8, buffer.as_mut_ptr(), size_of_val(&vm1))}
    add_data(&mut pefile, &buffer);

    // todo generate code that replaces original code (in this case there is none)
    // via dynasm referring to bytecode section with offset and to vm section
    // for turning bytecode into machine that runs
    // see loader for dynasm usage

    // rewriting entry point to data

    println!("{:x}", machine_addr + offset_of!(VmMachine, vmenter) as u32);
    println!("{:x}", machine_addr as u32 );
    let patched_entry = pefile.offset_to_rva(Offset(target_fn_addr as _)).unwrap();
    let nt_headers = pefile.get_valid_mut_nt_headers();
    assert!(nt_headers.is_ok());



    //

    pefile.recreate_image(PEType::Disk).unwrap();
    pefile.save("../hello_world/target/release/hello_world_modded.exe").unwrap();
}

fn patch_function(pefile: &mut VecPE, target_fn: usize, target_fn_size: usize, vm_rva: u32, bytecode_rva: u32) -> usize {
/*
    for index in 0..pefile.get_buffer().len() {
        if index >= target_fn && index <= target_fn + target_fn_size {
            pefile.remove(index);
        }
    }
 */
    let target_fn_rva = pefile.offset_to_rva(Offset(target_fn as u32)).unwrap();
    let mut a = CodeAssembler::new(64).unwrap();
    // todo add relocs
    // todo get the fcking right address!!
/*
    let mut push = Decoder::new(64, &vec![0xff, 0x35, 0x00, 0x00, 0x00, 0x00], 0).decode();
    push.set_memory_displacement64(bytecode_rva as u64 - target_fn_rva.0 as u64);
    a.add_instruction(push).unwrap();
 */
    println!("{:x}",  pefile.get_image_base().unwrap());
    println!("rva: {:x}", pefile.get_image_base().unwrap() + vm_rva as u64  as u64);
    a.jmp(vm_rva as u64 - target_fn_rva.0 as u64).unwrap();
    let patch = a.assemble(0).unwrap();

    println!("{:x}", target_fn_rva.0);

    let target_function_mut = pefile.get_mut_slice_ref::<u8>(target_fn, patch.len()).unwrap();
    target_function_mut.copy_from_slice(patch.as_slice());
    //pefile.get_mut_section_by_name(".text".to_string()).unwrap().
    pefile.pad_to_alignment().unwrap();
    pefile.fix_image_size().unwrap();
    patch.len()
}

fn add_section(pefile: &mut VecPE, section: &ImageSectionHeader, data: &Vec<u8>) -> Result<ImageSectionHeader, Error> {
    let new_section = pefile.append_section(section)?.clone();
    pefile.append(data);
    pefile.pad_to_alignment().unwrap();
    pefile.fix_image_size().unwrap();
    Ok(new_section)
}

fn add_data(pefile: &mut VecPE, data: &Vec<u8>) {
    pefile.append(data);
    pefile.pad_to_alignment().unwrap();
    pefile.fix_image_size().unwrap();
}


// goal is to generate one interpreter (run function)
// then generate multiple machine instances here
// maybe have to add some relocs

#[repr(C)]
pub struct VmMachine {
    pub(crate) pc: *const u8,
    pub(crate) sp: *mut u64,
    pub regs: [u64; 16],
    pub(crate) program: [u8; 166],
    pub(crate) vmstack: Vec<u64>,
    pub(crate) cpustack: Vec<u8>,
    vmenter: [u8; 88],
    vmexit: [u8; 64],
}

fn create_machine(program_ptr: u64, program_len: usize) -> VmMachine {
    VmMachine {
        pc: std::ptr::null(),
        sp: std::ptr::null_mut(),
        regs: [0; 16],
        program: [0; 166],
        vmstack: [0; 0x1000].to_vec(),
        cpustack: [0; 0x1000].to_vec(),
        vmenter: [0; 88],
        vmexit: [0; 64],
    }
}

fn generate_vm_entry(m: &mut VmMachine, machine_addr: u64, run_addr: usize) -> Vec<u8> {
    use iced_x86::code_asm::*;
    // Generate VMENTER.
    let regmap: &[(&AsmRegister64, u8)] = &[
        (&rax, Register::Rax.into()),
        (&rcx, Register::Rcx.into()),
        (&rdx, Register::Rdx.into()),
        (&rbx, Register::Rbx.into()),
        (&rsp, Register::Rsp.into()),
        (&rbp, Register::Rbp.into()),
        (&rsi, Register::Rsi.into()),
        (&rdi, Register::Rdi.into()),
        (&r8, Register::R8.into()),
        (&r9, Register::R9.into()),
        (&r10, Register::R10.into()),
        (&r11, Register::R11.into()),
        (&r12, Register::R12.into()),
        (&r13, Register::R13.into()),
        (&r14, Register::R14.into()),
        (&r15, Register::R15.into()),
    ];

    // thanks to cursey <3 :3 ^-^ >~<
    // remove this, place it into main.rs or something
    // wat i mean is pre assemble the vmenter and vmexit
    // instead of assembling it here
    // so i also dont need to allocate the regions
    // they will be stack/ in data section / in bytecode section
    // allocated maybe, to test just get the output from this code below
    // and replace vmenter and vmexit with the arrays
    // check in pe-bear for relocations!!
    let mut a = CodeAssembler::new(64).unwrap();

    a.mov(rax, rcx).unwrap();

    // Store the GPRs
    for (reg, regid) in regmap.iter() {
        let offset = offset_of!(VmMachine, regs) + *regid as usize * 8;
        a.mov(qword_ptr(rax + offset), **reg).unwrap();
    }

    // Switch to the VM's CPU stack.
   /*
    let vm_rsp = unsafe {
        m.cpustack
            .as_ptr()
            .add(m.cpustack.len() - 0x100 - std::mem::size_of::<u64>()) as u64
    };
    */
    // this wouldnt work since i have to get a ptr to the inner buf of vec
    let vm_rsp = qword_ptr(
        rax + offset_of!(VmMachine, cpustack) + m.cpustack.len() - 0x100 - std::mem::size_of::<u64>()
    );

    a.mov(rsp, vm_rsp).unwrap();

    a.mov(rcx, rax).unwrap();
    a.mov(rax, rdx).unwrap();
    a.jmp(rax).unwrap();

    let vm_enter = a.assemble(m.vmenter.as_ptr() as u64).unwrap();
    m.vmenter.copy_from_slice(&vm_enter);
    m.vmenter.clone().to_vec()
}

fn generate_vm_exit(m: &mut VmMachine) -> Vec<u8> {
    use iced_x86::code_asm::*;
    // Generate VMEXIT.
    let regmap: &[(&AsmRegister64, u8)] = &[
        (&rax, Register::Rax.into()),
        (&rbx, Register::Rbx.into()),
        (&rsp, Register::Rsp.into()),
        (&rbp, Register::Rbp.into()),
        (&rsi, Register::Rsi.into()),
        (&rdi, Register::Rdi.into()),
        (&r8, Register::R8.into()),
        (&r9, Register::R9.into()),
        (&r10, Register::R10.into()),
        (&r11, Register::R11.into()),
        (&r12, Register::R12.into()),
        (&r13, Register::R13.into()),
        (&r14, Register::R14.into()),
        (&r15, Register::R15.into()),
    ];

    // look above, same applies here
    let mut a = CodeAssembler::new(64).unwrap();

    // Restore the GPRs
    for (reg, regid) in regmap.iter() {
        let offset = offset_of!(VmMachine, regs) + *regid as usize * 8;
        a.mov(**reg, qword_ptr(rcx + offset)).unwrap();
    }

    a.jmp(rdx).unwrap();

    let vm_exit = a.assemble(m.vmexit.as_ptr() as u64).unwrap();
    m.vmexit.copy_from_slice(&vm_exit);
    m.vmexit.clone().to_vec()
}