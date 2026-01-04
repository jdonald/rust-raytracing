use glam::{Vec3, Mat4};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub color: [f32; 3], // Basic vertex color
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Material {
    pub color: [f32; 4],
    pub params: [f32; 4], // x: type, y: roughness, z: ior, w: sss_amount
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SceneDesc {
    pub vertex_addr: u64,
    pub index_addr: u64,
    pub material_addr: u64,
}

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

pub struct SceneObject {
    pub mesh_index: usize,
    pub transform: Mat4,
    pub material_index: usize,
}

pub struct Scene {
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub objects: Vec<SceneObject>,
}

impl Scene {
    pub fn new() -> Self {
        let mut scene = Scene {
            meshes: Vec::new(),
            materials: Vec::new(),
            objects: Vec::new(),
        };

        // Materials
        // 0: Gray Concrete
        scene.materials.push(Material { color: [0.5, 0.5, 0.5, 1.0], params: [0.0, 1.0, 0.0, 0.0] }); 
        // 1: Green Leaves
        scene.materials.push(Material { color: [0.1, 0.8, 0.1, 1.0], params: [0.0, 1.0, 0.0, 0.0] });
        // 2: Brown Bark
        scene.materials.push(Material { color: [0.4, 0.2, 0.1, 1.0], params: [0.0, 1.0, 0.0, 0.0] });
        // 3: Red Brick (House)
        scene.materials.push(Material { color: [0.8, 0.3, 0.2, 1.0], params: [0.0, 1.0, 0.0, 0.0] });
        // 4: Blue Car (Metallic)
        scene.materials.push(Material { color: [0.2, 0.2, 0.9, 1.0], params: [1.0, 0.2, 0.0, 0.0] });
        // 5: Glass (Window)
        scene.materials.push(Material { color: [1.0, 1.0, 1.0, 1.0], params: [2.0, 0.0, 1.5, 0.0] });
        // 6: Water (Puddle)
        scene.materials.push(Material { color: [0.8, 0.8, 1.0, 1.0], params: [1.0, 0.05, 1.33, 0.0] }); // Metallic/Dielectric hybrid in shader?
        // 7: Skin (SSS)
        scene.materials.push(Material { color: [0.9, 0.7, 0.6, 1.0], params: [3.0, 0.5, 0.0, 1.0] });
        // 8: Asphalt
        scene.materials.push(Material { color: [0.2, 0.2, 0.2, 1.0], params: [0.0, 1.0, 0.0, 0.0] });

        // Geometry Generation
        let cube = create_cube();
        let sphere = create_sphere(16, 16);
        
        scene.meshes.push(cube); // 0
        scene.meshes.push(sphere); // 1

        // Ground (Asphalt)
        scene.objects.push(SceneObject {
            mesh_index: 0,
            transform: Mat4::from_scale_rotation_translation(Vec3::new(20.0, 0.1, 20.0), Default::default(), Vec3::new(0.0, -0.1, 0.0)),
            material_index: 8,
        });

        // Puddle (Flat Cube slightly above ground)
        scene.objects.push(SceneObject {
            mesh_index: 0,
            transform: Mat4::from_scale_rotation_translation(Vec3::new(3.0, 0.05, 3.0), Default::default(), Vec3::new(5.0, -0.05, 2.0)),
            material_index: 6,
        });

        // House
        // Body
        scene.objects.push(SceneObject {
            mesh_index: 0,
            transform: Mat4::from_scale_rotation_translation(Vec3::new(4.0, 3.0, 4.0), Default::default(), Vec3::new(-5.0, 1.5, -5.0)),
            material_index: 3,
        });
        // Window
        scene.objects.push(SceneObject {
            mesh_index: 0,
            transform: Mat4::from_scale_rotation_translation(Vec3::new(1.0, 1.0, 0.1), Default::default(), Vec3::new(-5.0, 1.5, -0.9)), // Front of house
            material_index: 5,
        });

        // Tree
        // Trunk
        scene.objects.push(SceneObject {
            mesh_index: 0, // Cube for now as trunk
            transform: Mat4::from_scale_rotation_translation(Vec3::new(0.5, 2.0, 0.5), Default::default(), Vec3::new(5.0, 1.0, -5.0)),
            material_index: 2,
        });
        // Leaves
        scene.objects.push(SceneObject {
            mesh_index: 1, // Sphere
            transform: Mat4::from_scale_rotation_translation(Vec3::new(2.0, 2.0, 2.0), Default::default(), Vec3::new(5.0, 3.0, -5.0)),
            material_index: 1,
        });

        // Car
        scene.objects.push(SceneObject {
            mesh_index: 0,
            transform: Mat4::from_scale_rotation_translation(Vec3::new(1.5, 0.5, 3.0), Default::default(), Vec3::new(2.0, 0.5, 5.0)),
            material_index: 4,
        });

        // Person
        scene.objects.push(SceneObject {
            mesh_index: 1, // Sphere head
            transform: Mat4::from_scale_rotation_translation(Vec3::new(0.3, 0.3, 0.3), Default::default(), Vec3::new(-2.0, 1.6, 2.0)),
            material_index: 7,
        });
        scene.objects.push(SceneObject {
            mesh_index: 0, // Cube body
            transform: Mat4::from_scale_rotation_translation(Vec3::new(0.4, 0.7, 0.2), Default::default(), Vec3::new(-2.0, 0.7, 2.0)),
            material_index: 0, // Clothes
        });

        scene
    }
}

fn create_cube() -> Mesh {
    let vertices = vec![
        // Front
        Vertex { pos: [-0.5, -0.5,  0.5], nrm: [ 0.0,  0.0,  1.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5, -0.5,  0.5], nrm: [ 0.0,  0.0,  1.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5,  0.5,  0.5], nrm: [ 0.0,  0.0,  1.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [-0.5,  0.5,  0.5], nrm: [ 0.0,  0.0,  1.0], color: [1.0, 1.0, 1.0] },
        // Back
        Vertex { pos: [-0.5, -0.5, -0.5], nrm: [ 0.0,  0.0, -1.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [-0.5,  0.5, -0.5], nrm: [ 0.0,  0.0, -1.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5,  0.5, -0.5], nrm: [ 0.0,  0.0, -1.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5, -0.5, -0.5], nrm: [ 0.0,  0.0, -1.0], color: [1.0, 1.0, 1.0] },
        // Top
        Vertex { pos: [-0.5,  0.5, -0.5], nrm: [ 0.0,  1.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [-0.5,  0.5,  0.5], nrm: [ 0.0,  1.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5,  0.5,  0.5], nrm: [ 0.0,  1.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5,  0.5, -0.5], nrm: [ 0.0,  1.0,  0.0], color: [1.0, 1.0, 1.0] },
        // Bottom
        Vertex { pos: [-0.5, -0.5, -0.5], nrm: [ 0.0, -1.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5, -0.5, -0.5], nrm: [ 0.0, -1.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5, -0.5,  0.5], nrm: [ 0.0, -1.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [-0.5, -0.5,  0.5], nrm: [ 0.0, -1.0,  0.0], color: [1.0, 1.0, 1.0] },
        // Right
        Vertex { pos: [ 0.5, -0.5, -0.5], nrm: [ 1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5,  0.5, -0.5], nrm: [ 1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5,  0.5,  0.5], nrm: [ 1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [ 0.5, -0.5,  0.5], nrm: [ 1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
        // Left
        Vertex { pos: [-0.5, -0.5, -0.5], nrm: [-1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [-0.5, -0.5,  0.5], nrm: [-1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [-0.5,  0.5,  0.5], nrm: [-1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
        Vertex { pos: [-0.5,  0.5, -0.5], nrm: [-1.0,  0.0,  0.0], color: [1.0, 1.0, 1.0] },
    ];
    let indices = vec![
        0, 1, 2, 0, 2, 3,
        4, 5, 6, 4, 6, 7,
        8, 9, 10, 8, 10, 11,
        12, 13, 14, 12, 14, 15,
        16, 17, 18, 16, 18, 19,
        20, 21, 22, 20, 22, 23
    ];
    Mesh { vertices, indices }
}

fn create_sphere(slices: u32, stacks: u32) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for i in 0..=stacks {
        let v = i as f32 / stacks as f32;
        let phi = v * std::f32::consts::PI;

        for j in 0..=slices {
            let u = j as f32 / slices as f32;
            let theta = u * std::f32::consts::PI * 2.0;

            let x = theta.cos() * phi.sin();
            let y = phi.cos();
            let z = theta.sin() * phi.sin();

            vertices.push(Vertex {
                pos: [x * 0.5, y * 0.5, z * 0.5],
                nrm: [x, y, z],
                color: [1.0, 1.0, 1.0],
            });
        }
    }

    for i in 0..stacks {
        for j in 0..slices {
            let first = (i * (slices + 1)) + j;
            let second = first + slices + 1;

            indices.push(first);
            indices.push(second);
            indices.push(first + 1);

            indices.push(second);
            indices.push(second + 1);
            indices.push(first + 1);
        }
    }
    Mesh { vertices, indices }
}
