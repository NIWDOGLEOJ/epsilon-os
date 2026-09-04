//! Bare-Metal In-Kernel Self-Tests & ISA Debug Exit for AegisOS
//!
//! Enabled conditionally via `--features selftest`. Executes at early boot to verify:
//! 1. Physical Bitmap Frame Allocator (allocation, alignment, zeroing, burst & recycle)
//! 2. 4-Level PML4 Paging & Address Space Isolation (user PML4, supervisor mirroring, mapping & translation, reclamation)
//! 3. Kernel Dynamic Heap Allocator (Box, Vec, String, coalescing integrity)
//! 4. Preemptive Task Scheduler & Lifecycle (process spawning, PID 0 immunity, termination & zombie reaping)
//!
//! Exits QEMU deterministically via `isa-debug-exit` (I/O port 0xf4):
//! - Success code: 0x10 (QEMU exit status (0x10 << 1) | 1 = 33 / 0x21)
//! - Failure code: 0x11 (QEMU exit status (0x11 << 1) | 1 = 35 / 0x23)

use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use crate::arch::serial::outb;
use crate::memory::{
    alloc_frame, alloc_zeroed_frame, create_user_address_space, destroy_user_address_space,
    free_frame, get_kernel_pml4, map_page, phys_to_virt, translate_addr, PageTable,
    PageTableFlags, VirtAddr, PAGE_SIZE,
};
use crate::serial_println;
use crate::task::{get_process_list, kill_process, reap_zombies, spawn_process, TaskState};

const DEBUG_EXIT_PORT: u16 = 0xf4;
const EXIT_SUCCESS_CODE: u8 = 0x10; // QEMU exit code: (16 << 1) | 1 = 33
const EXIT_FAILURE_CODE: u8 = 0x11; // QEMU exit code: (17 << 1) | 1 = 35

extern "C" fn selftest_dummy_worker() {
    loop {
        core::hint::spin_loop();
    }
}

/// Runs all in-kernel bare-metal self-test suites and signals QEMU exit.
pub fn run_kernel_selftests() {
    serial_println!("=======================================================");
    serial_println!("        AegisOS In-Kernel Bare-Metal Self-Tests        ");
    serial_println!("=======================================================");

    let mut passed_suites = 0;
    let total_suites = 14;

    // 1. Physical Frame Allocator Test Suite
    serial_println!("[SELFTEST:1/14] Running Physical Frame Allocator Tests...");
    if let Err(e) = test_physical_frame_allocator() {
        fail_and_exit("Physical Frame Allocator", e);
    }
    serial_println!("[SELFTEST:1/14] [PASS] Physical Frame Allocator Suite OK.");
    passed_suites += 1;

    // 2. PML4 Paging & Address Space Isolation Test Suite
    serial_println!("[SELFTEST:2/14] Running PML4 Paging & Isolation Tests...");
    if let Err(e) = test_pml4_paging_and_isolation() {
        fail_and_exit("PML4 Paging & Isolation", e);
    }
    serial_println!("[SELFTEST:2/14] [PASS] PML4 Paging & Isolation Suite OK.");
    passed_suites += 1;

    // 3. Kernel Dynamic Heap Allocator Test Suite
    serial_println!("[SELFTEST:3/14] Running Kernel Dynamic Heap Tests...");
    if let Err(e) = test_kernel_heap() {
        fail_and_exit("Kernel Dynamic Heap", e);
    }
    serial_println!("[SELFTEST:3/14] [PASS] Kernel Dynamic Heap Suite OK.");
    passed_suites += 1;

    // 4. Preemptive Task Scheduler & Process Lifecycle Suite
    serial_println!("[SELFTEST:4/14] Running Task Scheduler Lifecycle Tests...");
    if let Err(e) = test_scheduler_lifecycle() {
        fail_and_exit("Task Scheduler Lifecycle", e);
    }
    serial_println!("[SELFTEST:4/14] [PASS] Task Scheduler Lifecycle Suite OK.");
    passed_suites += 1;

    // 5. In-Memory Virtual Filesystem (RAM Disk VFS) Suite
    serial_println!("[SELFTEST:5/14] Running In-Memory VFS Tests...");
    if let Err(e) = test_virtual_filesystem() {
        fail_and_exit("In-Memory VFS", e);
    }
    serial_println!("[SELFTEST:5/14] [PASS] In-Memory VFS Suite OK.");
    passed_suites += 1;

    // 6. Hardware PC Speaker & Audio Subsystem Suite
    serial_println!("[SELFTEST:6/14] Running PC Speaker Audio Tests...");
    if let Err(e) = test_pc_speaker_driver() {
        fail_and_exit("PC Speaker Audio", e);
    }
    serial_println!("[SELFTEST:6/14] [PASS] PC Speaker Audio Suite OK.");
    passed_suites += 1;

    // 7. Desktop Wallpaper Engine & PPM Image Parser Suite
    serial_println!("[SELFTEST:7/14] Running Wallpaper & PPM Parser Tests...");
    if let Err(e) = test_wallpaper_and_ppm_parser() {
        fail_and_exit("Wallpaper & PPM Parser", e);
    }
    serial_println!("[SELFTEST:7/14] [PASS] Wallpaper & PPM Parser Suite OK.");
    passed_suites += 1;

    // 8. Scientific Calculator 2.0 Engine & Math Functions Suite
    serial_println!("[SELFTEST:8/14] Running Scientific Calculator Tests...");
    if let Err(e) = test_scientific_calculator_engine() {
        fail_and_exit("Scientific Calculator", e);
    }
    serial_println!("[SELFTEST:8/14] [PASS] Scientific Calculator Suite OK.");
    passed_suites += 1;

    // 9. Terminal 2.0 Engine (History, Tab Auto-Completion & ANSI Codes)
    serial_println!("[SELFTEST:9/14] Running Terminal 2.0 Engine Tests...");
    if let Err(e) = test_terminal_engine() {
        fail_and_exit("Terminal 2.0 Engine", e);
    }
    serial_println!("[SELFTEST:9/14] [PASS] Terminal 2.0 Engine Suite OK.");
    passed_suites += 1;

    // 10. AI Agent Kernel Bridge, Spotlight Search & Aegis Browser
    serial_println!("[SELFTEST:10/14] Running AI Agent, Spotlight & Browser Tests...");
    if let Err(e) = test_agent_spotlight_browser_engine() {
        fail_and_exit("AI Agent, Spotlight & Browser", e);
    }
    serial_println!("[SELFTEST:10/14] [PASS] AI Agent, Spotlight & Browser Suite OK.");
    passed_suites += 1;

    // 11. Minesweeper Retro Arcade Game Suite
    serial_println!("[SELFTEST:11/14] Running Minesweeper Retro Arcade Tests...");
    if let Err(e) = test_minesweeper_engine() {
        fail_and_exit("Minesweeper Retro Arcade", e);
    }
    serial_println!("[SELFTEST:11/14] [PASS] Minesweeper Retro Arcade Suite OK.");
    passed_suites += 1;

    // 12. AegisPad 2.0 Advanced Multi-Tab Editor Suite
    serial_println!("[SELFTEST:12/14] Running AegisPad 2.0 Advanced Editor Tests...");
    if let Err(e) = test_editor_advanced_engine() {
        fail_and_exit("AegisPad 2.0 Advanced Editor", e);
    }
    serial_println!("[SELFTEST:12/14] [PASS] AegisPad 2.0 Advanced Editor Suite OK.");
    passed_suites += 1;

    // 13. AegisSynth Chiptune Synthesizer & Pattern Sequencer Suite
    serial_println!("[SELFTEST:13/14] Running AegisSynth Chiptune Studio Tests...");
    if let Err(e) = test_synth_engine() {
        fail_and_exit("AegisSynth Chiptune Studio", e);
    }
    serial_println!("[SELFTEST:13/14] [PASS] AegisSynth Chiptune Studio Suite OK.");
    passed_suites += 1;

    // 14. In-Kernel Virtual Network Stack & AegisChat Suite
    serial_println!("[SELFTEST:14/14] Running Virtual Network & AegisChat Tests...");
    if let Err(e) = test_network_loopback_and_chat_engine() {
        fail_and_exit("Virtual Network & AegisChat", e);
    }
    serial_println!("[SELFTEST:14/14] [PASS] Virtual Network & AegisChat Suite OK.");
    passed_suites += 1;

    serial_println!("=======================================================");
    serial_println!(
        "[SELFTEST:PASS] All bare-metal in-kernel unit tests passed! ({}/{} suites)",
        passed_suites,
        total_suites
    );
    serial_println!(
        "[SELFTEST:EXIT] Signaling QEMU isa-debug-exit on port 0x{:x} with code 0x{:x}...",
        DEBUG_EXIT_PORT,
        EXIT_SUCCESS_CODE
    );
    serial_println!("=======================================================");

    unsafe {
        outb(DEBUG_EXIT_PORT, EXIT_SUCCESS_CODE);
    }

    // Halt the CPU while QEMU processes the debug exit
    crate::hcf();
}

fn fail_and_exit(suite_name: &str, error_msg: &str) -> ! {
    serial_println!("=======================================================");
    serial_println!(
        "[SELFTEST:FAIL] In-Kernel Self-Test Suite '{}' FAILED!",
        suite_name
    );
    serial_println!("[SELFTEST:FAIL] Reason: {}", error_msg);
    serial_println!(
        "[SELFTEST:EXIT] Signaling QEMU isa-debug-exit failure on port 0x{:x}...",
        DEBUG_EXIT_PORT
    );
    serial_println!("=======================================================");

    unsafe {
        outb(DEBUG_EXIT_PORT, EXIT_FAILURE_CODE);
    }

    crate::hcf();
}

/// 1. Physical Bitmap Frame Allocator Verification
fn test_physical_frame_allocator() -> Result<(), &'static str> {
    // A. Allocate single frame
    let frame1 = alloc_frame().ok_or("alloc_frame() returned None")?;
    if !frame1.is_aligned_4k() {
        return Err("Allocated frame is not 4KB aligned");
    }
    if frame1.is_null() {
        return Err("Allocated frame returned null physical address (0x0)");
    }

    // B. Memory Write & Read Integrity via HHDM
    let virt_ptr = phys_to_virt(frame1).as_mut_ptr::<u64>();
    unsafe {
        core::ptr::write_volatile(virt_ptr, 0x1122_3344_5566_7788u64);
        let read_val = core::ptr::read_volatile(virt_ptr);
        if read_val != 0x1122_3344_5566_7788u64 {
            return Err("Memory readback mismatch on allocated physical frame");
        }
    }

    // C. Allocate zeroed frame
    let zeroed_frame = alloc_zeroed_frame().ok_or("alloc_zeroed_frame() returned None")?;
    let zero_ptr = phys_to_virt(zeroed_frame).as_ptr::<u8>();
    unsafe {
        for i in 0..PAGE_SIZE {
            if core::ptr::read_volatile(zero_ptr.add(i)) != 0 {
                return Err("alloc_zeroed_frame() produced non-zero byte");
            }
        }
    }

    // D. Burst allocation of 64 distinct frames
    let mut burst_frames = Vec::with_capacity(64);
    for _ in 0..64 {
        let f = alloc_frame().ok_or("Out of memory during 64-frame burst allocation")?;
        if burst_frames.contains(&f) {
            return Err("Duplicate physical frame allocated during burst");
        }
        burst_frames.push(f);
    }

    // E. Free all burst frames
    for f in burst_frames {
        free_frame(f);
    }

    // Free initial test frames
    free_frame(frame1);
    free_frame(zeroed_frame);

    // F. Frame Recycling: Re-allocate a frame and confirm success
    let recycled_frame = alloc_frame().ok_or("alloc_frame() failed after freeing burst frames")?;
    free_frame(recycled_frame);

    Ok(())
}

/// 2. PML4 Virtual Address Space & Page Table Isolation
fn test_pml4_paging_and_isolation() -> Result<(), &'static str> {
    // A. Create new isolated user address space
    let user_pml4 = create_user_address_space();
    if user_pml4.is_null() || !user_pml4.is_aligned_4k() {
        return Err("create_user_address_space returned invalid PML4 physical address");
    }

    let kernel_pml4 = get_kernel_pml4();
    let user_table = unsafe { &*phys_to_virt(user_pml4).as_ptr::<PageTable>() };
    let kernel_table = unsafe { &*phys_to_virt(kernel_pml4).as_ptr::<PageTable>() };

    // B. Lower-Half (entries 0..256) must be completely unmapped for user isolation
    for idx in 0..256 {
        if user_table.entries[idx].is_present() {
            return Err("User PML4 lower-half (0..255) contains unexpected mapped entry");
        }
    }

    // C. Higher-Half (entries 256..512) must strictly mirror supervisor kernel mappings
    for idx in 256..512 {
        if user_table.entries[idx] != kernel_table.entries[idx] {
            return Err("User PML4 higher-half (256..511) does not mirror kernel PML4");
        }
    }

    // D. Map a test page in user address space
    let test_virt = VirtAddr::new(0x0000_0000_5000_0000);
    let test_phys = alloc_zeroed_frame().ok_or("alloc_zeroed_frame failed for user test page")?;

    map_page(
        user_pml4,
        test_virt,
        test_phys,
        PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::USER_ACCESSIBLE,
    );

    // E. Verify virtual-to-physical address translation
    let translated = translate_addr(user_pml4, test_virt);
    if translated != Some(test_phys) {
        return Err("translate_addr failed to resolve mapped user page");
    }

    // F. Destroy user address space and verify reclamation
    let reclaimed_count = unsafe { destroy_user_address_space(user_pml4) };
    // Should reclaim at least: PT frame, leaf frame, and root PML4 frame (>= 3 frames)
    if reclaimed_count < 3 {
        return Err("destroy_user_address_space reclaimed fewer frames than expected");
    }

    Ok(())
}

/// 3. Kernel Dynamic Heap Allocator Verification
fn test_kernel_heap() -> Result<(), &'static str> {
    // A. Box allocation
    let boxed_val = Box::new(0xCAFE_BABE_DEAD_BEEFu64);
    if *boxed_val != 0xCAFE_BABE_DEAD_BEEFu64 {
        return Err("Box heap readback value mismatch");
    }
    drop(boxed_val);

    // B. Vec dynamic growth and arithmetic integrity
    let mut test_vec = Vec::new();
    for i in 0..1000 {
        test_vec.push(i as u64);
    }
    if test_vec.len() != 1000 {
        return Err("Vec did not reach expected length of 1000");
    }
    let sum: u64 = test_vec.iter().sum();
    // Sum of 0..999 = (999 * 1000) / 2 = 499500
    if sum != 499_500 {
        return Err("Vec elements sum mismatch");
    }
    drop(test_vec);

    // C. String dynamic formatting
    let s = format!("AegisOS-Kernel-SelfTest-{}", 42);
    if s != "AegisOS-Kernel-SelfTest-42" {
        return Err("Dynamic String format mismatch");
    }
    drop(s);

    Ok(())
}

/// 4. Task Scheduler and Process Lifecycle Verification
fn test_scheduler_lifecycle() -> Result<(), &'static str> {
    // A. Assert PID 0 [idle] task exists
    let initial_procs = get_process_list();
    if initial_procs.is_empty() {
        return Err("Process list is empty");
    }
    let idle_proc = initial_procs
        .iter()
        .find(|p| p.pid == 0)
        .ok_or("PID 0 [idle] task not found in process table")?;
    if idle_proc.name != "[idle]" {
        return Err("PID 0 task does not have name '[idle]'");
    }

    // B. Spawn a test worker task
    let worker_pid = spawn_process("selftest_worker", selftest_dummy_worker, false);
    if worker_pid <= 0 {
        return Err("spawn_process returned invalid PID");
    }

    // C. Verify worker task appears in process list
    let procs_after_spawn = get_process_list();
    let worker_found = procs_after_spawn.iter().any(|p| p.pid == worker_pid);
    if !worker_found {
        return Err("Spawned worker task not present in process table");
    }

    // D. Terminate worker task
    let killed = kill_process(worker_pid);
    if !killed {
        return Err("kill_process() returned false for active worker PID");
    }

    // E. Reap zombies
    reap_zombies();

    // F. Verify worker task is terminated
    let procs_after_reap = get_process_list();
    if let Some(worker) = procs_after_reap.iter().find(|p| p.pid == worker_pid) {
        match worker.state {
            TaskState::Terminated(_) | TaskState::Zombie => {
                // Expected
            }
            _ => {
                return Err("Worker process not marked terminated after kill");
            }
        }
    }

    Ok(())
}

/// 5. In-Memory Virtual Filesystem (RAM Disk VFS) Verification
fn test_virtual_filesystem() -> Result<(), &'static str> {
    // A. Verify seed document exists
    if !crate::fs::file_exists("/welcome.txt") {
        return Err("Pre-seeded file '/welcome.txt' missing from VFS");
    }

    let welcome = crate::fs::read_to_string("/welcome.txt")
        .map_err(|_| "Failed to read '/welcome.txt'")?;
    if !welcome.contains("Welcome to AegisPad") {
        return Err("Seed file '/welcome.txt' contents corrupted");
    }

    // B. Create a temporary file and write data
    let test_path = "/selftest_tmp.txt";
    let test_data = b"AegisOS-VFS-SelfTest-Data-Payload-12345";
    crate::fs::write_file(test_path, test_data)
        .map_err(|_| "Failed to write '/selftest_tmp.txt'")?;

    // C. Verify file exists
    if !crate::fs::file_exists(test_path) {
        return Err("Written file does not exist in VFS");
    }

    // D. Read data back and verify exact byte match
    let read_back = crate::fs::read_to_string(test_path)
        .map_err(|_| "Failed to read back '/selftest_tmp.txt'")?;
    if read_back.as_bytes() != test_data {
        return Err("VFS readback data mismatch");
    }

    // E. Remove file and assert deletion
    crate::fs::remove_file(test_path)
        .map_err(|_| "Failed to remove '/selftest_tmp.txt'")?;
    if crate::fs::file_exists(test_path) {
        return Err("Removed file still exists in VFS");
    }

    Ok(())
}

/// Test 6: Hardware PC Speaker & Audio Subsystem Test Suite
fn test_pc_speaker_driver() -> Result<(), &'static str> {
    use crate::drivers::speaker::{
        is_speaker_active, mute, read_speaker_port, set_frequency, Note, AUDIO_PLAYER,
    };

    // A. Verify Initial Mute State
    mute();
    let port_after_mute = read_speaker_port();
    if (port_after_mute & 0x03) != 0 {
        return Err("Mute did not clear Port 0x61 bits 0 and 1");
    }
    if is_speaker_active() {
        return Err("Speaker reported active while muted");
    }

    // B. Test Frequency Programming (440 Hz)
    set_frequency(440);
    let port_after_tone = read_speaker_port();
    if (port_after_tone & 0x03) != 0x03 {
        return Err("set_frequency did not assert Port 0x61 bits 0 and 1");
    }
    if !is_speaker_active() {
        return Err("Speaker reported inactive after set_frequency");
    }

    // C. Mute again
    mute();
    if is_speaker_active() {
        return Err("Speaker remains active after secondary mute");
    }

    // D. Test AudioPlayer step sequencing
    {
        let mut player = AUDIO_PLAYER.lock();
        player.clear();
        player.enqueue_notes(&[Note::new(880, 2)]);
        player.step();
        if player.active_freq != 880 {
            return Err("AudioPlayer did not activate enqueued tone on first step");
        }
        player.step(); // frame 2
        player.step(); // note expired, should mute
        if player.active_freq != 0 {
            return Err("AudioPlayer did not mute after tone frames expired");
        }
    }

    mute();
    Ok(())
}

/// Self-test Suite 7: Desktop Wallpaper Engine & PPM Image Parser
fn test_wallpaper_and_ppm_parser() -> Result<(), &'static str> {
    // A. Parse valid P6 PPM image
    let valid_ppm = b"P6\n# Test synthetic PPM image\n2 2\n255\n\xFF\x00\x00\x00\xFF\x00\x00\x00\xFF\xFF\xFF\xFF";
    let ppm = crate::gui::wallpaper::parse_ppm_p6(valid_ppm)?;

    if ppm.width != 2 || ppm.height != 2 {
        return Err("PPM parsed incorrect dimensions (expected 2x2)");
    }
    if ppm.pixels.len() != 4 {
        return Err("PPM parsed incorrect pixel count (expected 4)");
    }
    // Pixel 0: Red
    if ppm.pixels[0] != crate::gui::primitives::Color::rgb(255, 0, 0) {
        return Err("PPM pixel (0,0) was not pure Red");
    }
    // Pixel 1: Green
    if ppm.pixels[1] != crate::gui::primitives::Color::rgb(0, 255, 0) {
        return Err("PPM pixel (1,0) was not pure Green");
    }
    // Pixel 2: Blue
    if ppm.pixels[2] != crate::gui::primitives::Color::rgb(0, 0, 255) {
        return Err("PPM pixel (0,1) was not pure Blue");
    }
    // Pixel 3: White
    if ppm.pixels[3] != crate::gui::primitives::Color::rgb(255, 255, 255) {
        return Err("PPM pixel (1,1) was not pure White");
    }

    // B. Test Error Handling on Corrupted PPM inputs
    // 1. Truncated pixel data
    let truncated_data = b"P6\n2 2\n255\n\xFF\x00";
    if crate::gui::wallpaper::parse_ppm_p6(truncated_data).is_ok() {
        return Err("PPM parser did not reject truncated pixel data");
    }

    // 2. Invalid magic number
    let bad_magic = b"P5\n2 2\n255\n\x00\x00\x00\x00";
    if crate::gui::wallpaper::parse_ppm_p6(bad_magic).is_ok() {
        return Err("PPM parser did not reject invalid magic header 'P5'");
    }

    // 3. Zero dimensions
    let zero_dim = b"P6\n0 0\n255\n";
    if crate::gui::wallpaper::parse_ppm_p6(zero_dim).is_ok() {
        return Err("PPM parser did not reject 0x0 dimensions");
    }

    // C. VFS Wallpaper persistence & round-trip verification
    let vfs_path = "/system/selftest_bg.ppm";
    crate::fs::write_file(vfs_path, valid_ppm)?;

    let read_back = crate::fs::read_file(vfs_path)?;
    let parsed_back = crate::gui::wallpaper::parse_ppm_p6(&read_back)?;
    if parsed_back.width != 2 || parsed_back.height != 2 {
        return Err("VFS round-trip PPM lost dimensions");
    }
    if parsed_back.pixels[0] != crate::gui::primitives::Color::rgb(255, 0, 0) {
        return Err("VFS round-trip PPM corrupted pixel data");
    }

    // Clean up
    crate::fs::remove_file(vfs_path)?;

    Ok(())
}

/// Self-test Suite 8: Scientific Calculator 2.0 Engine & Math Functions
fn test_scientific_calculator_engine() -> Result<(), &'static str> {
    use crate::apps::calculator::{CalcOp, CalculatorApp};

    // A. Newton-Raphson float square root solver
    let sqrt_144 = CalculatorApp::compute_sqrt(144.0);
    if (sqrt_144 - 12.0).abs() > 0.0001 {
        return Err("compute_sqrt(144.0) did not converge to 12.0");
    }
    let sqrt_0 = CalculatorApp::compute_sqrt(0.0);
    if sqrt_0 != 0.0 {
        return Err("compute_sqrt(0.0) was not 0.0");
    }
    let sqrt_2 = CalculatorApp::compute_sqrt(2.0);
    if (sqrt_2 - 1.41421356).abs() > 0.0001 {
        return Err("compute_sqrt(2.0) did not converge to ~1.4142");
    }

    // B. Binary power exponentiation
    let pow_2_8 = CalculatorApp::compute_power(2.0, 8.0);
    if (pow_2_8 - 256.0).abs() > 0.0001 {
        return Err("compute_power(2.0, 8.0) did not equal 256.0");
    }
    let pow_5_0 = CalculatorApp::compute_power(5.0, 0.0);
    if (pow_5_0 - 1.0).abs() > 0.0001 {
        return Err("compute_power(5.0, 0.0) did not equal 1.0");
    }
    let pow_10_3 = CalculatorApp::compute_power(10.0, 3.0);
    if (pow_10_3 - 1000.0).abs() > 0.0001 {
        return Err("compute_power(10.0, 3.0) did not equal 1000.0");
    }

    // C. CalculatorApp Interactive State Machine & History Tape
    let mut calc = CalculatorApp::new();
    // Input 45
    calc.input_digit('4');
    calc.input_digit('5');
    if calc.display != "45" {
        return Err("Calculator failed to buffer digits '45'");
    }

    // Set '+' operator
    calc.set_operator(CalcOp::Add);
    if (calc.accumulator - 45.0).abs() > 0.0001 {
        return Err("Calculator accumulator did not capture 45.0 on operator");
    }

    // Input 55
    calc.input_digit('5');
    calc.input_digit('5');
    if calc.display != "55" {
        return Err("Calculator failed to buffer digits '55'");
    }

    // Equals: 45 + 55 = 100
    calc.equals();
    if calc.display != "100" {
        return Err("Calculator 45 + 55 did not evaluate to '100'");
    }
    if calc.history.len() != 1 {
        return Err("Calculator history tape did not record evaluation");
    }
    if (calc.history[0].result - 100.0).abs() > 0.0001 {
        return Err("Calculator history tape entry result did not equal 100.0");
    }

    // Scientific sqrt: sqrt(100) = 10
    calc.sqrt();
    if calc.display != "10" {
        return Err("Calculator sqrt(100) did not evaluate to '10'");
    }
    if calc.history.len() != 2 {
        return Err("Calculator history tape did not record sqrt operation");
    }

    // Reciprocal: 1/10 = 0.1
    calc.reciprocal();
    if calc.display != "0.1" {
        return Err("Calculator 1/10 did not evaluate to '0.1'");
    }

    // History recall: recall item 0 (100)
    calc.recall_history(0);
    if calc.display != "100" {
        return Err("Calculator failed to recall item from history tape");
    }

    // D. Error handling: Divide by Zero
    calc.clear();
    calc.input_digit('1');
    calc.input_digit('0');
    calc.set_operator(CalcOp::Divide);
    calc.input_digit('0');
    calc.equals();
    if !calc.is_error || calc.display != "Error" {
        return Err("Calculator did not enter error state on divide by zero");
    }

    // E. Error handling: Negative Square Root
    calc.clear();
    calc.input_digit('4');
    calc.toggle_sign();
    calc.sqrt();
    if !calc.is_error || calc.display != "Error" {
        return Err("Calculator did not enter error state on sqrt(-4)");
    }

    Ok(())
}

/// Self-test Suite 9: Terminal 2.0 Engine (History, Tab Completion & ANSI)
fn test_terminal_engine() -> Result<(), &'static str> {
    use alloc::string::ToString;
    use crate::apps::terminal::{strip_ansi, TerminalApp};
    use crate::drivers::ps2_keyboard::{KeyCode, KeyEvent};

    let mut term = TerminalApp::new();

    // A. Command history navigation & draft preservation
    term.command_history.push("neofetch".to_string());
    term.command_history.push("ls".to_string());
    term.command_history.push("cat /welcome.txt".to_string());

    let make_key = |code: KeyCode| KeyEvent {
        code,
        char_byte: None,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
        caps: false,
        scancode: 0,
    };

    // User types a draft: "my draft"
    term.input_buffer = "my draft".to_string();

    // Up Arrow: should save draft and recall latest command ("cat /welcome.txt")
    term.handle_key(make_key(KeyCode::Up));
    if term.input_buffer != "cat /welcome.txt" {
        return Err("Terminal Up arrow did not recall latest command");
    }
    if term.saved_draft != "my draft" {
        return Err("Terminal did not preserve draft on Up arrow");
    }

    // Up Arrow again: should recall "ls"
    term.handle_key(make_key(KeyCode::Up));
    if term.input_buffer != "ls" {
        return Err("Terminal Up arrow did not recall previous command 'ls'");
    }

    // Down Arrow: should recall "cat /welcome.txt"
    term.handle_key(make_key(KeyCode::Down));
    if term.input_buffer != "cat /welcome.txt" {
        return Err("Terminal Down arrow did not advance to next command");
    }

    // Down Arrow again: should restore draft "my draft"
    term.handle_key(make_key(KeyCode::Down));
    if term.input_buffer != "my draft" {
        return Err("Terminal Down arrow did not restore saved draft");
    }

    // B. Tab Auto-Completion
    // Test 1: Command prefix completion ("wallp" -> "wallpaper ")
    term.input_buffer = "wallp".to_string();
    term.auto_complete();
    if term.input_buffer != "wallpaper " {
        return Err("Tab auto-completion failed for command 'wallp'");
    }

    // Test 2: App name completion ("run set" -> "run settings ")
    term.input_buffer = "run set".to_string();
    term.auto_complete();
    if term.input_buffer != "run settings " {
        return Err("Tab auto-completion failed for app 'run set'");
    }

    // Test 3: VFS path completion ("cat /wel" -> "cat /welcome.txt ")
    term.input_buffer = "cat /wel".to_string();
    term.auto_complete();
    if term.input_buffer != "cat /welcome.txt " {
        return Err("Tab auto-completion failed for VFS path 'cat /wel'");
    }

    // Test 4: Longest common prefix algorithm
    let candidates = [
        "terminal".to_string(),
        "terminate".to_string(),
        "terms".to_string(),
    ];
    let lcp = TerminalApp::find_longest_common_prefix(&candidates);
    if lcp != "term" {
        return Err("find_longest_common_prefix did not return 'term'");
    }

    // C. ANSI Escape Sequence parsing and code stripping
    let styled = "\x1b[1;32maegis\x1b[0m:\x1b[1;34m~\x1b[0m$ ";
    let clean = strip_ansi(styled);
    if clean != "aegis:~$ " {
        return Err("strip_ansi failed to strip colored prompt");
    }

    let err_styled = "\x1b[1;31mError: unknown command\x1b[0m";
    let err_clean = strip_ansi(err_styled);
    if err_clean != "Error: unknown command" {
        return Err("strip_ansi failed to strip error message");
    }

    Ok(())
}

/// Self-test Suite 10: AI Agent Kernel Bridge, Spotlight Search & Aegis Browser
fn test_agent_spotlight_browser_engine() -> Result<(), &'static str> {
    // A. AI Agent Kernel Bridge Protocol
    let ping_resp = crate::agent::handle_agent_packet("AGENT:PING");
    if !ping_resp.contains("RING_0_SUPERVISOR") {
        return Err("Agent ping failed to return RING_0_SUPERVISOR mode");
    }

    let sysinfo_resp = crate::agent::handle_agent_packet("AGENT:SYSINFO");
    if !sysinfo_resp.contains("cpu_percent") || !sysinfo_resp.contains("memory_used_mb") {
        return Err("Agent sysinfo did not contain expected telemetry metrics");
    }

    let write_resp = crate::agent::handle_agent_packet("AGENT:VFS_WRITE /agent_selftest.txt autonomous_supervisor");
    if !write_resp.contains("OK") {
        return Err("Agent VFS_WRITE failed");
    }

    let read_resp = crate::agent::handle_agent_packet("AGENT:VFS_READ /agent_selftest.txt");
    if !read_resp.contains("autonomous_supervisor") {
        return Err("Agent VFS_READ failed to read written content");
    }

    let (packets, vfs_ops, _, _) = crate::agent::get_agent_metrics();
    if packets < 4 || vfs_ops < 2 {
        return Err("Agent telemetry metrics did not track packet and VFS counts");
    }

    // B. Spotlight Universal Desktop Search
    let mut spot = crate::gui::spotlight::Spotlight::new();
    spot.toggle();
    if !spot.is_visible {
        return Err("Spotlight toggle did not make modal visible");
    }

    // App search
    spot.push_char('c');
    spot.push_char('a');
    spot.push_char('l');
    spot.push_char('c');
    let has_calc = spot.results.iter().any(|r| match r {
        crate::gui::spotlight::SearchResult::App(id, _) => *id == crate::gui::dock::AppId::Calculator,
        _ => false,
    });
    if !has_calc {
        return Err("Spotlight search for 'calc' did not return Calculator App");
    }

    // Math search (inline sqrt)
    spot.query.clear();
    spot.push_char('s');
    spot.push_char('q');
    spot.push_char('r');
    spot.push_char('t');
    spot.push_char('(');
    spot.push_char('1');
    spot.push_char('4');
    spot.push_char('4');
    spot.push_char(')');
    let has_math_12 = spot.results.iter().any(|r| match r {
        crate::gui::spotlight::SearchResult::MathResult(_, val) => (*val - 12.0).abs() < 0.001,
        _ => false,
    });
    if !has_math_12 {
        return Err("Spotlight inline math eval for 'sqrt(144)' did not return 12.0");
    }

    // C. Aegis Hypertext Web Browser
    let mut browser = crate::apps::browser::BrowserApp::new();
    if browser.current_url != "aegis://home" {
        return Err("Browser did not start at aegis://home");
    }
    if browser.rendered_lines.is_empty() {
        return Err("Browser home page has no rendered content lines");
    }

    // Navigate to agent dashboard
    browser.navigate("aegis://agent");
    if browser.current_url != "aegis://agent" {
        return Err("Browser failed to navigate to aegis://agent");
    }
    let has_agent_header = browser.rendered_lines.iter().any(|l| l.text.contains("AI Agent"));
    if !has_agent_header {
        return Err("Browser aegis://agent page did not render AI Agent header");
    }

    // Navigate to VFS document
    browser.navigate("vfs:///welcome.txt");
    if browser.current_url != "vfs:///welcome.txt" {
        return Err("Browser failed to navigate to vfs:///welcome.txt");
    }

    // Test back/forward history navigation
    browser.go_back();
    if browser.current_url != "aegis://agent" {
        return Err("Browser go_back did not return to aegis://agent");
    }

    browser.go_forward();
    if browser.current_url != "vfs:///welcome.txt" {
        return Err("Browser go_forward did not return to vfs:///welcome.txt");
    }

    Ok(())
}

/// Self-test Suite 11: Minesweeper Retro Arcade Game Engine
fn test_minesweeper_engine() -> Result<(), &'static str> {
    use crate::apps::minesweeper::{Difficulty, GameState, MinesweeperApp};

    let mut app = MinesweeperApp::new();

    // 1. Initial State verification
    if app.cols != 9 || app.rows != 9 || app.total_mines != 10 {
        return Err("Minesweeper default board dimensions or mine count incorrect");
    }
    if app.game_state != GameState::Ready {
        return Err("Minesweeper did not start in GameState::Ready");
    }
    if app.flags_count != 0 {
        return Err("Minesweeper initial flags count should be 0");
    }

    // 2. First-click safety verification (4, 4)
    app.reveal_cell(4, 4);
    if app.game_state != GameState::Playing {
        return Err("Minesweeper failed to transition to GameState::Playing after first click");
    }

    let c44 = &app.grid[4 * 9 + 4];
    if !c44.revealed || c44.has_mine {
        return Err("First clicked cell must be revealed and must not contain a mine");
    }

    // Verify none of the 8 neighbors of (4, 4) contain a mine
    for dr in -1isize..=1 {
        for dc in -1isize..=1 {
            let nc = 4isize + dc;
            let nr = 4isize + dr;
            let ni = (nr as usize) * 9 + (nc as usize);
            if app.grid[ni].has_mine {
                return Err("First click 3x3 safety zone violated: mine found in neighbor cell");
            }
        }
    }

    // Verify total mines placed on grid is exactly 10
    let mine_count = app.grid.iter().filter(|c| c.has_mine).count();
    if mine_count != 10 {
        return Err("Total mines placed on grid did not equal 10");
    }

    // 3. Flag toggling verification
    let mut target_cell = None;
    for r in 0..app.rows {
        for c in 0..app.cols {
            let i = r * app.cols + c;
            if !app.grid[i].revealed {
                target_cell = Some((c, r));
                break;
            }
        }
        if target_cell.is_some() {
            break;
        }
    }

    let (tc, tr) = target_cell.ok_or("No unrevealed cell available for flag test")?;
    let ti = tr * app.cols + tc;

    app.toggle_flag(tc, tr);
    if !app.grid[ti].flagged || app.flags_count != 1 {
        return Err("Flag toggle did not flag cell or increment flags count");
    }

    app.toggle_flag(tc, tr);
    if app.grid[ti].flagged || app.flags_count != 0 {
        return Err("Flag toggle did not unflag cell or decrement flags count");
    }

    // 4. Difficulty switching
    app.set_difficulty(Difficulty::Intermediate);
    if app.cols != 16 || app.rows != 16 || app.total_mines != 40 {
        return Err("Difficulty switch to Intermediate failed to configure 16x16 / 40 mines");
    }
    if app.game_state != GameState::Ready {
        return Err("Difficulty switch did not reset game to Ready");
    }

    app.set_difficulty(Difficulty::Beginner);
    if app.cols != 9 || app.rows != 9 || app.total_mines != 10 {
        return Err("Difficulty switch back to Beginner failed to configure 9x9 / 10 mines");
    }

    Ok(())
}

/// Self-test Suite 12: AegisPad 2.0 Multi-Tab Syntax & Code Editor Engine
fn test_editor_advanced_engine() -> Result<(), &'static str> {
    use crate::apps::editor::EditorApp;

    let mut app = EditorApp::new();

    // 1. Initial tab state
    if app.tabs.len() != 1 || app.active_tab != 0 {
        return Err("Editor did not initialize with exactly 1 active tab");
    }
    if app.active_tab().title != "welcome.txt" {
        return Err("Editor initial tab title should be welcome.txt");
    }

    // 2. New Tab Creation
    app.new_tab("/user/test.rs", Some("fn main() {\n    let x = 42;\n}"));
    if app.tabs.len() != 2 || app.active_tab != 1 {
        return Err("Editor failed to create and activate second tab");
    }
    if app.active_tab().title != "test.rs" || app.active_tab().lines.len() != 3 {
        return Err("Editor second tab content or title incorrect");
    }

    // 3. Search & Find Engine
    app.find_active = true;
    app.find_query = "let".into();
    app.update_find_matches();
    if app.find_matches.is_empty() {
        return Err("Editor find engine failed to locate 'let' in active tab");
    }
    let (mr, mc) = app.find_matches[0];
    if mr != 1 || mc != 4 {
        return Err("Editor find match coordinates incorrect for 'let'");
    }

    // 4. Syntax Tokenization
    if !EditorApp::is_keyword("fn") || !EditorApp::is_keyword("let") || !EditorApp::is_keyword("struct") {
        return Err("Editor is_keyword failed on standard Rust keywords");
    }
    if EditorApp::is_keyword("variable") || EditorApp::is_keyword("hello") {
        return Err("Editor is_keyword returned true on non-keyword identifiers");
    }

    // 5. Tab Closing
    app.close_tab(1);
    if app.tabs.len() != 1 || app.active_tab != 0 {
        return Err("Editor close_tab failed to clean up closed tab and reset active_tab");
    }

    Ok(())
}

/// Self-test Suite 13: AegisSynth Chiptune Synthesizer & Pattern Sequencer
fn test_synth_engine() -> Result<(), &'static str> {
    use crate::apps::synth::{SynthApp, NOTE_A4, NOTE_C4, NOTE_C5};

    // 1. Note frequency definitions
    if NOTE_A4 != 440 || NOTE_C4 != 262 || NOTE_C5 != 523 {
        return Err("Standard musical pitch frequency definitions incorrect");
    }

    let mut app = SynthApp::new();

    // 2. Initial state
    if app.is_playing {
        return Err("Synth sequencer should initialize in stopped state");
    }
    if app.bpm != 120 {
        return Err("Synth default tempo should be 120 BPM");
    }
    if app.current_step != 0 {
        return Err("Synth default step should be 0");
    }

    // 3. Step toggling
    let orig = app.pattern[0][5];
    app.toggle_step(0, 5);
    if app.pattern[0][5] == orig {
        return Err("toggle_step failed to invert pattern cell trigger");
    }
    app.toggle_step(0, 5);
    if app.pattern[0][5] != orig {
        return Err("toggle_step failed to return pattern cell trigger to original state");
    }

    // 4. Pattern Clear
    app.clear_pattern();
    for track in 0..4 {
        for step in 0..16 {
            if app.pattern[track][step] {
                return Err("clear_pattern failed to reset all triggers to false");
            }
        }
    }

    // 5. Preset Loading
    app.load_preset(1); // Mario
    if !app.pattern[0][0] || !app.pattern[0][2] || !app.pattern[0][4] {
        return Err("load_preset failed to load Mario pattern triggers");
    }

    // 6. Sequencer playhead tick
    app.is_playing = true;
    app.last_tick = 100;
    app.tick_sequencer(200); // 100 ticks elapsed > (1500 / 120 = 12 ticks)
    if app.current_step != 1 {
        return Err("Sequencer playhead failed to advance step on timer tick");
    }

    Ok(())
}

/// Self-test Suite 14: In-Kernel Virtual Network Stack & AegisChat
fn test_network_loopback_and_chat_engine() -> Result<(), &'static str> {
    use crate::apps::chat::ChatApp;
    use crate::net::{Ipv4Address, Ipv4Header, UdpHeader, UdpSocket};

    // 1. IPv4 Address & Loopback Verification
    let loopback = Ipv4Address::LOOPBACK;
    if !loopback.is_loopback() {
        return Err("IPv4 LOOPBACK (127.0.0.1) failed is_loopback() check");
    }
    if loopback.to_string() != "127.0.0.1" {
        return Err("IPv4 to_string() did not format 127.0.0.1 correctly");
    }

    // 2. IPv4 Header Serialization & RFC 791 Checksum
    let ip_hdr = Ipv4Header::new(
        Ipv4Address::LOOPBACK,
        Ipv4Address::LOOPBACK,
        Ipv4Header::PROTO_UDP,
        16,
    );
    if ip_hdr.checksum == 0 {
        return Err("IPv4 RFC 791 checksum calculation returned 0");
    }
    let ip_bytes = ip_hdr.serialize();
    let parsed_ip = Ipv4Header::parse(&ip_bytes).ok_or("Failed to parse serialized IPv4 header")?;
    if parsed_ip.src_ip != Ipv4Address::LOOPBACK || parsed_ip.protocol != Ipv4Header::PROTO_UDP {
        return Err("Parsed IPv4 header contents mismatch");
    }

    // 3. UDP Header Serialization & Parsing
    let udp_hdr = UdpHeader::new(8080, 8080, 16);
    let udp_bytes = udp_hdr.serialize();
    let parsed_udp = UdpHeader::parse(&udp_bytes).ok_or("Failed to parse serialized UDP header")?;
    if parsed_udp.src_port != 8080 || parsed_udp.dst_port != 8080 {
        return Err("Parsed UDP header ports mismatch");
    }

    // 4. Socket Bind, Transmit, and Loopback Receive
    let sock = UdpSocket::bind(8080);
    let test_payload = b"#general|guest|selftest loopback test packet";
    let sent = sock
        .send_to(Ipv4Address::LOOPBACK, 8080, test_payload)
        .map_err(|_| "UdpSocket send_to failed")?;
    if sent != test_payload.len() {
        return Err("UdpSocket sent byte count mismatch");
    }

    let (src_ip, src_port, recv_payload) = sock
        .recv_from()
        .ok_or("UdpSocket recv_from returned None for loopback packet")?;
    if src_ip != Ipv4Address::LOOPBACK || src_port != 8080 {
        return Err("Received packet addressing metadata mismatch");
    }
    if recv_payload != test_payload {
        return Err("Received payload does not match transmitted payload");
    }

    // 5. Chat Application & Autonomous AI Coprocessor Responses
    let agent_status_reply = ChatApp::generate_agent_response("@agent status");
    if !agent_status_reply.contains("System nominal") {
        return Err("ChatApp AI Coprocessor failed to generate status response");
    }
    let agent_mem_reply = ChatApp::generate_agent_response("@agent memory");
    if !agent_mem_reply.contains("16 MB") {
        return Err("ChatApp AI Coprocessor failed to generate memory response");
    }

    Ok(())
}




