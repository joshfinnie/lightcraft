//! Give memory the system allocator keeps after frees back to the system
//! ([`lightcraft_engine::memory::set_release_hook`]). The platform call lives in
//! `lightcraft-sysmem`, the workspace's only crate allowed `unsafe`.

/// Install the hooks, and limit image sizes to what this machine's memory can develop.
pub fn install() {
    lightcraft_engine::memory::set_release_hook(release);
    lightcraft_engine::memory::set_physical_memory_hook(lightcraft_sysmem::physical_memory);
    lightcraft_engine::memory::limit_images_to_machine();
}

fn release() {
    let _ = lightcraft_sysmem::release_free_memory();
}
