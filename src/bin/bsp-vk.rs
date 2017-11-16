#![feature(manually_drop)]

extern crate byteorder;
extern crate cgmath;
extern crate env_logger;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate num;
#[macro_use]
extern crate num_derive;
extern crate regex;
extern crate richter;
#[macro_use]
extern crate vulkano;
#[macro_use]
extern crate vulkano_shader_derive;
extern crate vulkano_win;
extern crate winit;

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::iter::IntoIterator;
use std::path::Path;
use std::sync::Arc;

use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use num::FromPrimitive;
use regex::Regex;
use vulkano::buffer::BufferAccess;
use vulkano::buffer::BufferSlice;
use vulkano::buffer::BufferUsage;
use vulkano::buffer::CpuAccessibleBuffer;
use vulkano::buffer::ImmutableBuffer;
use vulkano::command_buffer::AutoCommandBufferBuilder;
use vulkano::command_buffer::DynamicState;
use vulkano::descriptor::descriptor_set::FixedSizeDescriptorSetsPool;
use vulkano::device::Device;
use vulkano::device::DeviceExtensions;
use vulkano::device::Queue;
use vulkano::format::R8G8B8A8Unorm;
use vulkano::framebuffer::Framebuffer;
use vulkano::framebuffer::Subpass;
use vulkano::image::AttachmentImage;
use vulkano::image::Dimensions;
use vulkano::image::ImmutableImage;
use vulkano::image::ImageUsage;
use vulkano::image::ImageLayout;
use vulkano::image::ImageViewAccess;
use vulkano::image::MipmapsCount;
use vulkano::instance::Features;
use vulkano::instance::Instance;
use vulkano::instance::PhysicalDevice;
use vulkano::instance::QueueFamily;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::pipeline::viewport::Viewport;
use vulkano::sampler::Filter;
use vulkano::sampler::MipmapMode;
use vulkano::sampler::Sampler;
use vulkano::sampler::SamplerAddressMode;
use vulkano::swapchain::PresentMode;
use vulkano::swapchain::Swapchain;
use vulkano::swapchain::SurfaceTransform;
use vulkano::sync::now;
use vulkano::sync::GpuFuture;
use vulkano_win::VkSurfaceBuild;

const VERSION: i32 = 29;

const MAX_HULLS: usize = 4;
const MAX_MODELS: usize = 256;
const MAX_LEAVES: usize = 32767;
const MAX_BRUSHES: usize = 4096;
const MAX_ENTITIES: usize = 1024;
const MAX_ENTSTRING: usize = 65536;
const MAX_PLANES: usize = 8192;
const MAX_NODES: usize = 32767;
const MAX_CLIPNODES: usize = 32767;
const MAX_VERTICES: usize = 65535;
const MAX_FACES: usize = 65535;
const MAX_MARKTEXINFOS: usize = 65535;
const MAX_TEXINFOS: usize = 4096;
const MAX_EDGES: usize = 256000;
const MAX_EDGELIST: usize = 512000;
const MAX_TEXTURES: usize = 0x200000;
const MAX_LIGHTMAP: usize = 0x100000;
const MAX_VISLIST: usize = 0x100000;

const PLANE_SIZE: usize = 20;
const NODE_SIZE: usize = 24;
const LEAF_SIZE: usize = 28;
const TEXINFO_SIZE: usize = 40;
const FACE_SIZE: usize = 20;
const CLIPNODE_SIZE: usize = 8;
const FACELIST_SIZE: usize = 2;
const EDGE_SIZE: usize = 4;
const EDGELIST_SIZE: usize = 4;
const MODEL_SIZE: usize = 64;
const VERTEX_SIZE: usize = 12;
const TEX_NAME_MAX: usize = 16;

const MIPLEVELS: usize = 4;
const NUM_AMBIENTS: usize = 4;
const MAX_LIGHTSTYLES: usize = 4;
const MAX_TEXTURE_FRAMES: usize = 10;

const ASCII_0: usize = '0' as usize;
const ASCII_9: usize = '9' as usize;
const ASCII_A: usize = 'A' as usize;
const ASCII_J: usize = 'J' as usize;
const ASCII_a: usize = 'a' as usize;
const ASCII_j: usize = 'j' as usize;

//   | +z                 | -y
//   |                    |
//   |_____ -y Quake ->   |_____ +x Vulkan
//   /                    /
//  /                    /
// / +x                 / -z
//
// Quake  [x, y, z] <-> [-z, -x, -y] Vulkan
// Vulkan [x, y, z] <-> [ y, -z, -x] Quake

mod vs {
    #[derive(VulkanoShader)]
    #[ty = "vertex"]
    #[src = "
    #version 450

    layout (location = 0) in vec3 v_position;

    layout (location = 0) out vec2 f_texcoord;

    layout (push_constant) uniform PushConstants {
        mat4 u_projection;
        vec3 s_vector;
        float s_offset;
        vec3 t_vector;
        float t_offset;
        float tex_w;
        float tex_h;
    } pcs;

    vec3 quake_to_vulkan(vec3 v) {
        return vec3(-v.y, -v.z, -v.x);
    }

    void main() {
        f_texcoord = vec2(
            (dot(v_position, pcs.s_vector) + pcs.s_offset) / pcs.tex_w,
            (dot(v_position, pcs.t_vector) + pcs.t_offset) / pcs.tex_h
        );
        gl_Position = pcs.u_projection * vec4(quake_to_vulkan(v_position), 1.0);
    }
    "]
    struct Dummy;
}

mod fs {
    #[derive(VulkanoShader)]
    #[ty = "fragment"]
    #[src = "
    #version 450

    layout (location = 0) in vec2 f_texcoord;

    layout (location = 0) out vec4 out_color;

    layout (set = 0, binding = 0) uniform sampler2D u_sampler;

    layout (push_constant) uniform PushConstants {
        mat4 u_projection;
        vec3 s_vector;
        float s_offset;
        vec3 t_vector;
        float t_offset;
        float tex_w;
        float tex_h;
    } pcs;

    void main() {
        out_color = texture(u_sampler, f_texcoord);
    }"]
    struct Dummy;
}

#[derive(Debug)]
pub enum BspError {
    Io(std::io::Error),
    Other(String),
}

impl BspError {
    fn with_msg<S>(msg: S) -> Self
    where
        S: AsRef<str>,
    {
        BspError::Other(msg.as_ref().to_owned())
    }
}

impl fmt::Display for BspError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            BspError::Io(ref err) => err.fmt(f),
            BspError::Other(ref msg) => write!(f, "{}", msg),
        }
    }
}

impl Error for BspError {
    fn description(&self) -> &str {
        match *self {
            BspError::Io(ref err) => err.description(),
            BspError::Other(ref msg) => &msg,
        }
    }
}

impl From<std::io::Error> for BspError {
    fn from(error: std::io::Error) -> Self {
        BspError::Io(error)
    }
}

#[derive(Copy, Clone)]
struct BasicVertex {
    v_position: [f32; 2],
}

impl_vertex!(BasicVertex, v_position);

enum LumpId {
    Entities = 0,
    Planes = 1,
    Textures = 2,
    Vertices = 3,
    Visibility = 4,
    Nodes = 5,
    TextureInfo = 6,
    Faces = 7,
    Lightmaps = 8,
    ClipNodes = 9,
    Leaves = 10,
    FaceList = 11,
    Edges = 12,
    EdgeList = 13,
    Models = 14,
    Count = 15,
}

struct BspLump {
    offset: usize,
    size: usize,
}

impl BspLump {
    fn from_i32s(offset: i32, size: i32) -> Result<BspLump, BspError> {
        if offset < 0 {
            return Err(BspError::with_msg("Lump offset less than zero"));
        }

        if size < 0 {
            return Err(BspError::with_msg("Lump size less than zero"));
        }

        Ok(BspLump {
            offset: offset as usize,
            size: size as usize,
        })
    }
}

#[derive(FromPrimitive)]
enum BspPlaneKind {
    X = 0,
    Y = 1,
    Z = 2,
    AnyX = 3,
    AnyY = 4,
    AnyZ = 5,
}

struct BspPlane {
    normal: [f32; 3],
    dist: f32,
    kind: BspPlaneKind,
}

struct BspTexture {
    name: String,
    width: usize,
    height: usize,
    mipmaps: [Vec<u8>; MIPLEVELS],
    next: Option<usize>,
}

struct BspVisibility {
    data: Vec<Vec<u8>>,
}

enum BspNodeChild {
    Node(usize),
    Leaf(usize),
}

struct BspNode {
    plane_id: usize,
    front: BspNodeChild,
    back: BspNodeChild,
    min: [i16; 3],
    max: [i16; 3],
    face_id: usize,
    face_count: usize,
}

struct BspTexInfo {
    s_vector: [f32; 3],
    s_offset: f32,
    t_vector: [f32; 3],
    t_offset: f32,
    tex_id: usize,
    animated: bool,
}

#[derive(Copy, Clone)]
enum BspFaceSide {
    Front,
    Back,
}

struct BspFace {
    plane_id: usize,
    side: BspFaceSide,
    edge_id: usize,
    edge_count: usize,
    texinfo_id: usize,
    light_styles: [u8; MAX_LIGHTSTYLES],
    lightmap_id: Option<usize>,
}

enum BspClipNodeCollision {
    Index(usize),
    False,
    True,
}

impl BspClipNodeCollision {
    fn try_from_i16(val: i16) -> Result<BspClipNodeCollision, BspError> {
        match val {
            x if x < -2 => Err(BspError::with_msg("Invalid clip node collision value")),
            -2 => Ok(BspClipNodeCollision::True),
            -1 => Ok(BspClipNodeCollision::False),
            x => Ok(BspClipNodeCollision::Index(x as usize)),
        }
    }
}

struct BspClipNode {
    plane_id: usize,
    front: BspClipNodeCollision,
    back: BspClipNodeCollision,
}

struct BspLeaf {
    contents: i32,
    vis_offset: Option<usize>,
    min: [i16; 3],
    max: [i16; 3],
    face_id: usize,
    face_count: usize,
    sounds: [u8; 4],
}

struct BspEdge {
    vertex_ids: [usize; 2],
}

enum BspEdgeDirection {
    Forward,
    Backward,
}

struct BspEdgeIndex {
    direction: BspEdgeDirection,
    index: usize,
}

struct BspModel {
    min: [f32; 3],
    max: [f32; 3],
    origin: [f32; 3],
    roots: [i32; MAX_HULLS],
    leaf_count: usize,
    face_id: usize,
    face_count: usize,
}

pub struct Bsp {
    entities: Vec<HashMap<String, String>>,
    planes: Vec<BspPlane>,
    textures: Vec<BspTexture>,
    vertices: Vec<[f32; 3]>,
    visibility: Vec<u8>,
    nodes: Vec<BspNode>,
    texinfo: Vec<BspTexInfo>,
    faces: Vec<BspFace>,
    lightmaps: Vec<u8>,
    clipnodes: Vec<BspClipNode>,
    leaves: Vec<BspLeaf>,
    facelist: Vec<usize>,
    edges: Vec<BspEdge>,
    edgelist: Vec<BspEdgeIndex>,
    models: Vec<BspModel>,
}

impl Bsp {
    pub fn load<P>(path: P) -> Result<Bsp, BspError>
    where
        P: AsRef<Path>,
    {
        let path_str = path.as_ref().to_str().unwrap();

        debug!("Opening {}", path.as_ref().to_str().unwrap());
        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(err) => {
                return Err(BspError::Other(
                    format!("Failed to open {:?}", path.as_ref()),
                ))
            }
        };

        let mut reader = BufReader::new(&mut file);

        /// The BSP file header consists only of the file format version number.
        let version = reader.read_i32::<LittleEndian>()?;
        if (version != VERSION) {
            error!(
                "Bad version number in {} (found {}, should be {})",
                path_str,
                version,
                VERSION
            );
            return Err(BspError::with_msg("Bad version number"));
        }

        /// This is followed by a series of "lumps" (as they are called in the Quake source code),
        /// which act as a directory into the BSP file data. There are 15 of these lumps, each
        /// consisting of a 32-bit offset (into the file data) and a 32-bit size (in bytes).
        let mut lumps = Vec::with_capacity(LumpId::Count as usize);
        for l in 0..(LumpId::Count as usize) {
            let offset = match reader.read_i32::<LittleEndian>()? {
                o if o < 0 => return Err(BspError::Other(format!("Invalid lump offset of {}", o))),
                o => o,
            };

            let size = match reader.read_i32::<LittleEndian>()? {
                o if o < 0 => return Err(BspError::Other(format!("Invalid lump size of {}", o))),
                o => o,
            };

            debug!(
                "Lump {:>2}: Offset = 0x{:>08x} | Size = 0x{:>08x}",
                l,
                offset,
                size
            );

            lumps.push(BspLump::from_i32s(offset, size).expect(
                "Failed to read lump",
            ));
        }

        /// # Entities
        /// Lump 0 points to the level entity data, which is stored in a JSON-like dictionary
        /// format. Entities are anonymous; they do not have names, only attributes. They are stored
        /// as follows:
        ///
        ///     {
        ///     "attribute0" "value0"
        ///     "attribute1" "value1"
        ///     "attribute2" "value2"
        ///     }
        ///     {
        ///     "attribute0" "value0"
        ///     "attribute1" "value1"
        ///     "attribute2" "value2"
        ///     }
        ///
        /// The newline character is `0x0A` (line feed). The entity data is stored as a
        /// null-terminated string (it ends when byte `0x00` is reached).
        let ent_lump = &lumps[LumpId::Entities as usize];
        reader.seek(SeekFrom::Start(ent_lump.offset as u64))?;
        let mut ent_data = Vec::with_capacity(MAX_ENTSTRING);
        reader.read_until(0x00, &mut ent_data)?;
        if ent_data.len() > MAX_ENTSTRING {
            return Err(BspError::with_msg("Entity data exceeds MAX_ENTSTRING"));
        }
        let ent_string =
            String::from_utf8(ent_data).expect("Failed to create string from entity data");
        let ent_lines: Vec<&str> = ent_string.split('\n').collect();
        let mut entities: Vec<HashMap<String, String>> = Vec::with_capacity(MAX_ENTITIES);
        let mut ent_lines_iter = ent_lines.iter();
        lazy_static! {
            // match strings of the form "KEY" "VALUE", capturing KEY and VALUE
            static ref KEYVAL_REGEX: Regex = Regex::new(r#"^"([a-z]+)"\s+"(.+)"$"#).unwrap();
        }
        loop {
            match ent_lines_iter.next() {
                None => break,
                Some(line) => {
                    match *line {
                        "\u{0}" => break,
                        "{" => (),
                        _ => {
                            return Err(BspError::Other(
                                format!("Entities must begin with '{{' (got {:?})", *line),
                            ))
                        }
                    }
                }
            }

            debug!("Adding new entity");
            let mut entity: HashMap<String, String> = HashMap::new();
            while let Some(&line) = ent_lines_iter.next() {
                if line == "}" {
                    entity.shrink_to_fit();
                    entities.push(entity);
                    break;
                }

                let groups = match KEYVAL_REGEX.captures(line) {
                    Some(g) => g,
                    None => {
                        return Err(BspError::Other(
                            format!("Invalid attribute syntax in entity list: {}", line),
                        ))
                    }
                };

                let key = groups[1].to_string();

                // keys beginning with an underscore are treated as comments, see
                // https://github.com/id-Software/Quake/blob/master/QW/server/pr_edict.c#L843-L844
                if key.chars().next().unwrap() == '_' {
                    continue;
                }

                let val = groups[2].to_string();

                debug!("Inserting new attribute: {} = {}", key, val);
                entity.insert(key, val);
            }
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (ent_lump.offset + ent_lump.size) as u64,
            ))?
        );

        /// # Planes
        ///
        /// Lump 1 points to the planes used to partition the map, stored in point-normal form as 4
        /// IEEE 754 single-precision floats. The first 3 floats form the normal vector for the
        /// plane, and the last float specifies the distance from the map origin along the line
        /// defined by the normal vector.
        let plane_lump = &lumps[LumpId::Planes as usize];
        reader.seek(SeekFrom::Start(plane_lump.offset as u64))?;
        assert_eq!(plane_lump.size % PLANE_SIZE, 0);
        let plane_count = plane_lump.size / PLANE_SIZE;
        if plane_count > MAX_PLANES {
            return Err(BspError::with_msg("Plane count exceeds MAX_PLANES"));
        }
        let mut planes = Vec::with_capacity(plane_count);
        for _ in 0..plane_count {
            planes.push(BspPlane {
                normal: [
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                ],
                dist: reader.read_f32::<LittleEndian>()?,
                kind: BspPlaneKind::from_i32(reader.read_i32::<LittleEndian>()?).unwrap(),
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (plane_lump.offset + plane_lump.size) as u64,
            ))?
        );

        /// # Textures
        let tex_lump = &lumps[LumpId::Textures as usize];
        reader.seek(SeekFrom::Start(tex_lump.offset as u64))?;
        /// The textures are preceded by a 32-bit integer count and a list of 32-bit integer
        /// offsets. The offsets are given in bytes from the beginning of the texture section (the
        /// offset given by the texture lump at the start of the file).
        ///
        let tex_count = reader.read_i32::<LittleEndian>()?;
        if tex_count < 0 || tex_count as usize > MAX_TEXTURES {
            return Err(BspError::with_msg("Invalid texture count"));
        }
        let tex_count = tex_count as usize;
        let mut tex_offsets = Vec::with_capacity(tex_count);
        for _ in 0..tex_count {
            tex_offsets.push(reader.read_i32::<LittleEndian>()? as usize);
        }

        let mut textures = Vec::with_capacity(tex_count);
        for t in 0..tex_count {
            /// The textures themselves consist of a 16-byte name field, a 32-bit integer width, a
            /// 32-bit integer height, and 4 32-bit mipmap offsets. These offsets are given in
            /// bytes from the beginning of the texture. Each mipmap has its dimensions halved
            /// (i.e. its area quartered) from the previous mipmap: the first is full size, the
            /// second 1/4, the third 1/16, and the last 1/64. Each byte represents one pixel and
            /// contains an index into `gfx/palette.lmp`.
            reader.seek(SeekFrom::Start((tex_lump.offset + tex_offsets[t]) as u64))?;
            let mut tex_name_bytes = [0u8; TEX_NAME_MAX];
            reader.read(&mut tex_name_bytes)?;
            let len = tex_name_bytes.iter().enumerate()
                .find(|&item| item.1 == &0)
                .unwrap_or((TEX_NAME_MAX, &0)).0;
            let tex_name = String::from_utf8(tex_name_bytes[..len].to_vec()).unwrap();

            debug!("Texture {id:>width$}: {name}",
                   id=t,
                   width=(tex_count as f32).log(10.0) as usize,
                   name=tex_name);

            let width = reader.read_u32::<LittleEndian>()? as usize;
            let height = reader.read_u32::<LittleEndian>()? as usize;

            let mut mip_offsets = [0usize; MIPLEVELS];
            for m in 0..MIPLEVELS {
                mip_offsets[m] = reader.read_u32::<LittleEndian>()? as usize;
            }

            let mut mipmaps = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            for m in 0..MIPLEVELS {
                let factor = 2usize.pow(m as u32);
                let mipmap_size = (width as usize / factor) * (height as usize / factor);
                let offset = tex_lump.offset + tex_offsets[t] + mip_offsets[m];
                reader.seek(SeekFrom::Start(offset as u64))?;
                (&mut reader).take(mipmap_size as u64).read_to_end(&mut mipmaps[m])?;
            }

            textures.push(BspTexture {
                name: tex_name,
                width: width,
                height: height,
                mipmaps: mipmaps,
                next: None,
            })
        }

        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (tex_lump.offset + tex_lump.size) as u64,
            ))?
        );

        /// # Texture sequencing
        ///
        /// Animated textures are stored as individual frames with no guarantee of being in the
        /// correct order. This means that animated textures must be sequenced when the map is
        /// loaded. Frames of animated textures have names beginning with `U+002B PLUS SIGN` (`+`).

        debug!("Sequencing textures");
        for t in 0..textures.len() {
            if !textures[t].name.starts_with("+") || textures[t].next.is_some() {
                continue;
            }

            debug!("Sequencing texture {}", textures[t].name);

            /// Each texture can have two animations of up to MAX_TEXTURE_FRAMES frames each.
            let mut anim1 = [None; MAX_TEXTURE_FRAMES];
            let mut anim2 = [None; MAX_TEXTURE_FRAMES];
            let mut anim1_len = 0;
            let mut anim2_len = 0;

            let mut frame_char = textures[t].name.chars().nth(1).expect(
                "Invalid texture name",
            ) as usize;

            /// The character following the plus sign determines whether the frame belongs to the
            /// first or second animation.
            match frame_char {
                /// If it is between `U+0030 DIGIT ZERO` (`0`) and `U+0039 DIGIT NINE` (`9`), then
                /// the character represents that texture's frame index in the first animation
                /// sequence.
                ASCII_0...ASCII_9 => {
                    anim1_len = frame_char - ASCII_0;
                    anim2_len = 0;
                    anim1[anim1_len] = Some(t);
                    anim1_len += 1;
                }

                /// If it is between `U+0041 LATIN CAPITAL LETTER A` (`A`) and `U+004A LATIN CAPITAL
                /// LETTER J`, or between `U+0061 LATIN SMALL LETTER A` (`a`) and `U+006A LATIN
                /// SMALL LETTER J`, then the character represents that texture's frame index in the
                /// second animation sequence as that letter's position in the English alphabet
                /// (that is, `A`/`a` correspond to 0 and `J`/`j` correspond to 9).
                ASCII_A...ASCII_J | ASCII_a...ASCII_j => {
                    if frame_char >= ASCII_a && frame_char <= ASCII_j {
                        frame_char -= ASCII_a - ASCII_A;
                    }
                    anim2_len = frame_char - ASCII_A;
                    anim1_len = 0;
                    anim2[anim2_len] = Some(t);
                    anim2_len += 1;
                }

                _ => return Err(BspError::with_msg(
                    format!("Invalid texture frame specifier: U+{:x}", frame_char)
                ))
            }

            for t2 in t + 1..textures.len() {
                // check if this texture has the same base name
                if !textures[t2].name.starts_with("+") ||
                    textures[t2].name[2..] != textures[t].name[2..]
                {
                    continue;
                }

                let mut frame_n_char = textures[t2].name.chars().nth(1).expect(
                    "Invalid texture name",
                ) as usize;

                match frame_n_char {
                    ASCII_0...ASCII_9 => {
                        frame_n_char -= ASCII_0;
                        anim1[frame_n_char] = Some(t2);
                        if frame_n_char + 1 > anim1_len {
                            anim1_len = frame_n_char + 1;
                        }
                    }

                    ASCII_A...ASCII_J | ASCII_a...ASCII_j => {
                        if frame_n_char >= ASCII_a && frame_n_char <= ASCII_j {
                            frame_n_char -= ASCII_a - ASCII_A;
                        }
                        frame_n_char -= ASCII_A;
                        anim2[frame_n_char] = Some(t2);
                        if frame_n_char + 1 > anim2_len {
                            anim2_len += 1;
                        }
                    }

                    _ => return Err(BspError::with_msg(
                        format!("Invalid texture frame specifier: U+{:x}", frame_n_char)
                    ))
                }
            }

            // TODO: add animation timing data

            for frame in 0..anim1_len {
                let mut tex2 = match anim1[frame] {
                    Some(t2) => t2,
                    None => return Err(BspError::with_msg(
                        format!("Missing frame {} of {}", frame, textures[t].name)
                    ))
                };

                textures[tex2].next = Some(anim1[(frame + 1) % anim1_len].unwrap());
            }

            for frame in 0..anim2_len {
                let mut tex2 = match anim2[frame] {
                    Some(t2) => t2,
                    None => return Err(BspError::with_msg(
                        format!("Missing frame {} of {}", frame, textures[t].name)
                    ))
                };

                textures[tex2].next = Some(anim2[(frame + 1) % anim2_len].unwrap());
            }
        }

        /// # Vertex positions
        ///
        /// The vertex positions are stored as 3-component vectors of `float`.
        let vert_lump = &lumps[LumpId::Vertices as usize];
        reader.seek(SeekFrom::Start(vert_lump.offset as u64))?;
        assert_eq!(vert_lump.size % VERTEX_SIZE, 0);
        let vert_count = vert_lump.size / VERTEX_SIZE;
        if vert_count > MAX_VERTICES {
            return Err(BspError::with_msg("Vertex count exceeds MAX_VERTICES"));
        }
        let mut vertices = Vec::with_capacity(vert_count);
        for _ in 0..vert_count {
            vertices.push(
                [
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                ],
            );
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (vert_lump.offset + vert_lump.size) as u64,
            ))?
        );

        /// # Visibility lists
        ///
        /// The visibility lists are simply stored as a series of run-length encoded bit strings.
        /// The total size of the visibility data is given by the lump size.
        let vis_lump = &lumps[LumpId::Visibility as usize];
        reader.seek(SeekFrom::Start(vis_lump.offset as u64))?;
        if vis_lump.size > MAX_VISLIST {
            return Err(BspError::with_msg(
                "Visibility data size exceeds MAX_VISLIST",
            ));
        }
        let mut vis_data = Vec::with_capacity(vis_lump.size);
        (&mut reader).take(vis_lump.size as u64).read_to_end(
            &mut vis_data,
        )?;
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (vis_lump.offset + vis_lump.size) as u64,
            ))?
        );

        /// # Nodes
        ///
        /// Nodes are stored with a 32-bit integer plane ID denoting which plane splits the node.
        /// This is followed by two 16-bit integers which point to the children in front and back of
        /// the plane. If the high bit is set, the ID points to a leaf node; if not, it points to
        /// another internal node.
        ///
        /// After the node IDs are a 16-bit integer face ID, which denotes the index of the first
        /// face in the face list that belongs to this node, and a 16-bit integer face count, which
        /// denotes the number of faces to draw starting with the face ID.
        let node_lump = &lumps[LumpId::Nodes as usize];
        reader.seek(SeekFrom::Start(node_lump.offset as u64))?;
        assert_eq!(node_lump.size % NODE_SIZE, 0);
        let node_count = node_lump.size / NODE_SIZE;
        if node_count > MAX_NODES {
            return Err(BspError::with_msg("Node count exceeds MAX_NODES"));
        }
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let plane_id = reader.read_i32::<LittleEndian>()?;
            if plane_id < 0 {
                return Err(BspError::with_msg("Invalid plane id"));
            }

            let front = match reader.read_i16::<LittleEndian>()? {
                f if (f >> 15) & 1 == 1 => BspNodeChild::Leaf(f as usize),
                f => BspNodeChild::Node(f as usize),
            };

            let back = match reader.read_i16::<LittleEndian>()? {
                b if (b >> 15) & 1 == 1 => BspNodeChild::Leaf(b as usize),
                b => BspNodeChild::Node(b as usize),
            };

            let min = [
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
            ];

            let max = [
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
            ];

            let face_id = reader.read_i16::<LittleEndian>()?;
            if face_id < 0 {
                return Err(BspError::with_msg("Invalid face id"));
            }

            let face_count = reader.read_u16::<LittleEndian>()?;
            if face_count as usize > MAX_FACES {
                return Err(BspError::with_msg("Invalid face count"));
            }

            nodes.push(BspNode {
                plane_id: plane_id as usize,
                front: front,
                back: back,
                min: min,
                max: max,
                face_id: face_id as usize,
                face_count: face_count as usize,
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (node_lump.offset + node_lump.size) as u64,
            ))?
        );

        let texinfo_lump = &lumps[LumpId::TextureInfo as usize];
        reader.seek(SeekFrom::Start(texinfo_lump.offset as u64))?;
        assert_eq!(texinfo_lump.size % TEXINFO_SIZE, 0);
        let texinfo_count = texinfo_lump.size / TEXINFO_SIZE;
        let mut texinfos = Vec::with_capacity(texinfo_count);
        for _ in 0..texinfo_count {
            texinfos.push(BspTexInfo {
                s_vector: [
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                ],
                s_offset: reader.read_f32::<LittleEndian>()?,
                t_vector: [
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                    reader.read_f32::<LittleEndian>()?,
                ],
                t_offset: reader.read_f32::<LittleEndian>()?,
                tex_id: match reader.read_i32::<LittleEndian>()? {
                    t if t < 0 || t as usize > tex_count => {
                        return Err(BspError::with_msg("Invalid texture ID"))
                    }
                    t => t as usize,
                },
                animated: match reader.read_i32::<LittleEndian>()? {
                    0 => false,
                    1 => true,
                    _ => return Err(BspError::with_msg("Invalid texture flags")),
                },
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (texinfo_lump.offset + texinfo_lump.size) as u64,
            ))?
        );

        let face_lump = &lumps[LumpId::Faces as usize];
        reader.seek(SeekFrom::Start(face_lump.offset as u64))?;
        assert_eq!(face_lump.size % FACE_SIZE, 0);
        let face_count = face_lump.size / FACE_SIZE;
        let mut faces = Vec::with_capacity(face_count);
        for _ in 0..face_count {
            let plane_id = reader.read_i16::<LittleEndian>()?;
            if plane_id < 0 || plane_id as usize > plane_count {
                return Err(BspError::with_msg("Invalid plane count"));
            }

            let side = match reader.read_i16::<LittleEndian>()? {
                0 => BspFaceSide::Front,
                1 => BspFaceSide::Back,
                _ => return Err(BspError::with_msg("Invalid face side")),
            };

            let edge_id = reader.read_i32::<LittleEndian>()?;
            if edge_id < 0 {
                return Err(BspError::with_msg("Invalid edge ID"));
            }

            let edge_count = reader.read_i16::<LittleEndian>()?;
            if edge_count < 3 {
                return Err(BspError::with_msg("Invalid edge count"));
            }

            let texinfo_id = reader.read_i16::<LittleEndian>()?;
            if texinfo_id < 0 || texinfo_id as usize > texinfo_count {
                return Err(BspError::with_msg("Invalid texinfo ID"));
            }

            let mut light_styles = [0; MAX_LIGHTSTYLES];
            for i in 0..light_styles.len() {
                light_styles[i] = reader.read_u8()?;
            }

            let lightmap_id = match reader.read_i32::<LittleEndian>()? {
                o if o < -1 => return Err(BspError::with_msg("Invalid lightmap offset")),
                -1 => None,
                o => Some(o as usize),
            };

            faces.push(BspFace {
                plane_id: plane_id as usize,
                side: side,
                edge_id: edge_id as usize,
                edge_count: edge_count as usize,
                texinfo_id: texinfo_id as usize,
                light_styles: light_styles,
                lightmap_id: lightmap_id,
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (face_lump.offset + face_lump.size) as u64,
            ))?
        );

        let lightmap_lump = &lumps[LumpId::Lightmaps as usize];
        reader.seek(SeekFrom::Start(lightmap_lump.offset as u64))?;
        let mut lightmaps = Vec::with_capacity(lightmap_lump.size);
        (&mut reader).take(lightmap_lump.size as u64).read_to_end(
            &mut lightmaps,
        )?;
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (lightmap_lump.offset + lightmap_lump.size) as u64,
            ))?
        );

        let clipnode_lump = &lumps[LumpId::ClipNodes as usize];
        reader.seek(SeekFrom::Start(clipnode_lump.offset as u64))?;
        assert_eq!(clipnode_lump.size % CLIPNODE_SIZE, 0);
        let clipnode_count = clipnode_lump.size / CLIPNODE_SIZE;
        let mut clipnodes = Vec::with_capacity(clipnode_count);
        for _ in 0..clipnode_count {
            clipnodes.push(BspClipNode {
                plane_id: match reader.read_i32::<LittleEndian>()? {
                    x if x < 0 => return Err(BspError::with_msg("Invalid plane id")),
                    x => x as usize,
                },
                front: BspClipNodeCollision::try_from_i16(reader.read_i16::<LittleEndian>()?)
                    .unwrap(),
                back: BspClipNodeCollision::try_from_i16(reader.read_i16::<LittleEndian>()?)
                    .unwrap(),
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader
                .seek(SeekFrom::Start(
                    (clipnode_lump.offset + clipnode_lump.size) as u64,
                ))
                .unwrap()
        );

        let leaf_lump = &lumps[LumpId::Leaves as usize];
        reader
            .seek(SeekFrom::Start(leaf_lump.offset as u64))
            .unwrap();
        assert_eq!(leaf_lump.size % LEAF_SIZE, 0);
        let leaf_count = leaf_lump.size / LEAF_SIZE;
        if leaf_count > MAX_LEAVES {
            return Err(BspError::with_msg("Leaf count exceeds MAX_LEAVES"));
        }
        let mut leaves = Vec::with_capacity(leaf_count);
        for _ in 0..leaf_count {
            let contents = reader.read_i32::<LittleEndian>()?;
            let vis_offset = match reader.read_i32::<LittleEndian>()? {
                x if x < -1 => return Err(BspError::with_msg("Invalid visibility data offset")),
                -1 => None,
                x => Some(x as usize),
            };
            let mut min = [
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
            ];
            let mut max = [
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
                reader.read_i16::<LittleEndian>()?,
            ];
            let face_id = reader.read_u16::<LittleEndian>()? as usize;
            let face_count = reader.read_u16::<LittleEndian>()? as usize;
            let mut sounds = [0u8; NUM_AMBIENTS];
            reader.read(&mut sounds).unwrap();
            leaves.push(BspLeaf {
                contents: contents,
                vis_offset: vis_offset,
                min: min,
                max: max,
                face_id: face_id,
                face_count: face_count,
                sounds: sounds,
            });
        }

        let facelist_lump = &lumps[LumpId::FaceList as usize];
        reader
            .seek(SeekFrom::Start(facelist_lump.offset as u64))
            .unwrap();
        assert_eq!(facelist_lump.size % FACELIST_SIZE, 0);
        let facelist_count = facelist_lump.size / FACELIST_SIZE;
        let mut facelist = Vec::with_capacity(facelist_count);
        for _ in 0..facelist_count {
            facelist.push(reader.read_u16::<LittleEndian>()? as usize);
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0)).unwrap(),
            reader
                .seek(SeekFrom::Start(
                    (facelist_lump.offset + facelist_lump.size) as u64,
                ))
                .unwrap()
        );

        /// # Edges
        ///
        /// The edges are stored as a pair of 16-bit integer vertex IDs.
        let edge_lump = &lumps[LumpId::Edges as usize];
        reader
            .seek(SeekFrom::Start(edge_lump.offset as u64))
            .unwrap();
        assert_eq!(edge_lump.size % EDGE_SIZE, 0);
        let edge_count = edge_lump.size / EDGE_SIZE;
        if edge_count > MAX_EDGES {
            return Err(BspError::with_msg("Edge count exceeds MAX_EDGES"));
        }
        let mut edges = Vec::with_capacity(edge_count);
        for _ in 0..edge_count {
            edges.push(BspEdge {
                vertex_ids: [
                    reader.read_u16::<LittleEndian>()? as usize,
                    reader.read_u16::<LittleEndian>()? as usize,
                ],
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0)).unwrap(),
            reader
                .seek(SeekFrom::Start((edge_lump.offset + edge_lump.size) as u64))
                .unwrap()
        );

        let edgelist_lump = &lumps[LumpId::EdgeList as usize];
        reader.seek(SeekFrom::Start(edgelist_lump.offset as u64))?;
        assert_eq!(edgelist_lump.size % EDGELIST_SIZE, 0);
        let edgelist_count = edgelist_lump.size / EDGELIST_SIZE;
        if edgelist_count > MAX_EDGELIST {
            return Err(BspError::with_msg("Edge list count exceeds MAX_EDGELIST"));
        }
        let mut edgelist = Vec::with_capacity(edgelist_count);
        for _ in 0..edgelist_count {
            edgelist.push(match reader.read_i32::<LittleEndian>()? {
                x if x >= 0 => BspEdgeIndex {
                    direction: BspEdgeDirection::Forward,
                    index: x as usize,
                },

                x if x < 0 => BspEdgeIndex {
                    direction: BspEdgeDirection::Backward,
                    index: -x as usize,
                },

                x => return Err(BspError::with_msg(format!("Invalid edge index {}", x))),
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0)).unwrap(),
            reader
                .seek(SeekFrom::Start(
                    (edgelist_lump.offset + edgelist_lump.size) as u64,
                ))
                .unwrap()
        );

        let model_lump = &lumps[LumpId::Models as usize];
        reader
            .seek(SeekFrom::Start(model_lump.offset as u64))
            .unwrap();
        assert_eq!(model_lump.size % MODEL_SIZE, 0);
        let model_count = model_lump.size / MODEL_SIZE;
        if model_count > MAX_MODELS {
            return Err(BspError::with_msg("Model count exceeds MAX_MODELS"));
        }
        let mut models = Vec::with_capacity(model_count);
        for _ in 0..model_count {
            let min = [
                reader.read_f32::<LittleEndian>()?,
                reader.read_f32::<LittleEndian>()?,
                reader.read_f32::<LittleEndian>()?,
            ];

            let max = [
                reader.read_f32::<LittleEndian>()?,
                reader.read_f32::<LittleEndian>()?,
                reader.read_f32::<LittleEndian>()?,
            ];

            let origin = [
                reader.read_f32::<LittleEndian>()?,
                reader.read_f32::<LittleEndian>()?,
                reader.read_f32::<LittleEndian>()?,
            ];

            let mut roots = [0; MAX_HULLS];
            for i in 0..roots.len() {
                roots[i] = reader.read_i32::<LittleEndian>()?;
            }

            let leaf_count = match reader.read_i32::<LittleEndian>()? {
                x if x < 0 => return Err(BspError::with_msg("Invalid leaf count")),
                x => x as usize,
            };

            let face_id = match reader.read_i32::<LittleEndian>()? {
                x if x < 0 => return Err(BspError::with_msg("Invalid face id")),
                x => x as usize,
            };

            let face_count = match reader.read_i32::<LittleEndian>()? {
                x if x < 0 => return Err(BspError::with_msg("Invalid face count")),
                x => x as usize,
            };

            models.push(BspModel {
                min: min,
                max: max,
                origin: origin,
                roots: roots,
                leaf_count: leaf_count,
                face_id: face_id,
                face_count: face_count,
            });
        }
        assert_eq!(
            reader.seek(SeekFrom::Current(0))?,
            reader.seek(SeekFrom::Start(
                (model_lump.offset + model_lump.size) as u64,
            ))?
        );

        Ok(Bsp {
            entities: entities,
            planes: planes,
            textures: textures,
            vertices: vertices,
            visibility: vis_data,
            nodes: nodes,
            texinfo: texinfos,
            faces: faces,
            lightmaps: lightmaps,
            clipnodes: clipnodes,
            leaves: leaves,
            facelist: facelist,
            edges: edges,
            edgelist: edgelist,
            models: models,
        })
    }
}

type VkBspPlane = BspPlane;

struct VkBspTexture {
    img: Arc<ImmutableImage<R8G8B8A8Unorm>>,
    next: Option<usize>,
}

struct VkBspVertex {
    v_position: [f32; 3],
}
impl_vertex!(VkBspVertex, v_position);

type VkBspNodeChild = BspNodeChild;
type VkBspNode = BspNode;
type VkBspTexInfo = BspTexInfo;
type VkBspFaceSide = BspFaceSide;

struct VkBspFace {
    plane_id: usize,
    side: VkBspFaceSide,
    index_slice: Arc<BufferSlice<[u32], Arc<ImmutableBuffer<[u32]>>>>,
    texinfo_id: usize,
    light_styles: [u8; MAX_LIGHTSTYLES],
    lightmap_id: Option<usize>,
}

type VkBspClipNodeCollision = BspClipNodeCollision;
type VkBspClipNode = BspClipNode;
type VkBspLeaf = BspLeaf;
type VkBspModel = BspModel;

struct VkBsp {
    entities: Vec<HashMap<String, String>>,
    planes: Vec<VkBspPlane>,
    textures: Vec<VkBspTexture>,
    vertex_buffer: Arc<ImmutableBuffer<[VkBspVertex]>>,
    index_buffer: Arc<ImmutableBuffer<[u32]>>,
    visibility: Vec<u8>,
    nodes: Vec<VkBspNode>,
    texinfo: Vec<VkBspTexInfo>,
    faces: Vec<VkBspFace>,
    lightmaps: Vec<u8>,
    clipnodes: Vec<VkBspClipNode>,
    leaves: Vec<VkBspLeaf>,
    facelist: Vec<usize>,
}

impl VkBsp {
    pub fn new(bsp: Bsp, device: Arc<Device>, queue: Arc<Queue>) -> Result<VkBsp, BspError> {
        let mut textures: Vec<VkBspTexture> = Vec::new();

        for tex in bsp.textures.iter() {
            let rgba = richter::engine::indexed_to_rgba(&tex.mipmaps[0]);

            // TODO: currently does not support mipmaps
            let (img, img_future) = ImmutableImage::from_iter(
                rgba.into_iter(),
                Dimensions::Dim2d {
                    width: tex.width as u32,
                    height: tex.height as u32,
                },
                R8G8B8A8Unorm,
                queue.clone(),
            ).unwrap();


            // TODO: chain these futures
            img_future.flush().expect(
                "Failed to flush texture creation operations",
            );

            textures.push(VkBspTexture {
                img: img,
                next: tex.next,
            });
        }

        let (vertex_buffer, vb_future) =
            ImmutableBuffer::from_iter(
                bsp.vertices.iter().map(|&v| VkBspVertex { v_position: v }),
                BufferUsage::vertex_buffer(),
                queue.clone(),
            ).unwrap();

        vb_future.flush().expect(
            "Failed to flush vertex buffer creation",
        );

        let (index_buffer, ib_future) = ImmutableBuffer::from_iter(
            bsp.edgelist.iter().map(|ref e| {
                bsp.edges[e.index].vertex_ids[match e.direction {
                                                  BspEdgeDirection::Forward => 0,
                                                  BspEdgeDirection::Backward => 1,
                                              }] as u32
            }),
            BufferUsage::index_buffer(),
            queue.clone(),
        ).unwrap();

        ib_future.flush().expect(
            "Failed to flush index buffer creation",
        );

        let mut faces = Vec::new();
        for face in bsp.faces.iter() {
            faces.push(VkBspFace {
                plane_id: face.plane_id,
                side: face.side,
                index_slice: Arc::new(
                    BufferSlice::from(index_buffer.clone().into_buffer_slice())
                        .slice(face.edge_id..face.edge_id + face.edge_count)
                        .unwrap(),
                ),
                texinfo_id: face.texinfo_id,
                light_styles: face.light_styles,
                lightmap_id: face.lightmap_id,
            });
        }

        Ok(VkBsp {
            entities: bsp.entities,
            planes: bsp.planes,
            textures: textures,
            vertex_buffer: vertex_buffer.clone(),
            index_buffer: index_buffer.clone(),
            visibility: bsp.visibility,
            nodes: bsp.nodes,
            texinfo: bsp.texinfo,
            faces: faces,
            lightmaps: bsp.lightmaps,
            clipnodes: bsp.clipnodes,
            leaves: bsp.leaves,
            facelist: bsp.facelist,
        })
    }
}

fn main() {
    env_logger::init().expect("Failed to initialize logger");

    let required_extensions = vulkano_win::required_extensions();

    let instance = match Instance::new(None, &required_extensions, None) {
        Ok(i) => i,
        Err(err) => panic!("Failed to build Vulkan instance: {}", err),
    };

    let physical_device = match PhysicalDevice::enumerate(&instance).next() {
        Some(p) => p,
        None => panic!("No available Vulkan devices"),
    };

    let mut events_loop = winit::EventsLoop::new();
    let window = winit::WindowBuilder::new()
        .build_vk_surface(&events_loop, instance.clone())
        .unwrap();

    let queue_family = physical_device
        .queue_families()
        .find(|&q| q.supports_graphics())
        .expect("No available graphical queues");

    let device_extensions = DeviceExtensions {
        khr_swapchain: true,
        ..DeviceExtensions::none()
    };

    let (device, mut queues) = Device::new(
        physical_device,
        &Features::none(),
        &device_extensions,
        [(queue_family, 0.5)].iter().cloned(),
    ).expect("Failed to create device");

    let queue = queues.next().expect("No queue available");

    let mut dimensions;

    let (mut swapchain, mut images) = {
        let capabilities = window.surface().capabilities(physical_device).expect(
            "Failed to retrieve render surface capabilities",
        );

        dimensions = capabilities.current_extent.unwrap();
        let alpha = capabilities
            .supported_composite_alpha
            .iter()
            .next()
            .unwrap();
        let format = capabilities.supported_formats[0].0;

        match Swapchain::new(
            device.clone(),
            window.surface().clone(),
            capabilities.min_image_count,
            format,
            dimensions,
            1,
            capabilities.supported_usage_flags,
            &queue,
            SurfaceTransform::Identity,
            alpha,
            PresentMode::Fifo,
            true,
            None,
        ) {
            Ok(s) => s,
            Err(err) => panic!("Failed to create swapchain: {:?}", err),
        }
    };

    let depth_buffer =
        AttachmentImage::transient(device.clone(), dimensions, vulkano::format::D16Unorm).unwrap();

    let render_pass = Arc::new(
        single_pass_renderpass!(device.clone(),
                                attachments: {
                                    color: {
                                        load: Clear,
                                        store: Store,
                                        format: swapchain.format(),
                                        samples: 1,
                                    },
                                    depth: {
                                        load: Clear,
                                        store: DontCare,
                                        format: vulkano::format::Format::D16Unorm,
                                        samples: 1,
                                    }
                                },
                                pass: {
                                    color: [color],
                                    depth_stencil: {depth}
                                }).unwrap(),
    );

    let vs = vs::Shader::load(device.clone()).unwrap();
    let fs = fs::Shader::load(device.clone()).unwrap();

    let mut pcs = vs::ty::PushConstants {
        u_projection: cgmath::perspective(
            cgmath::Deg(75.0f32),
            dimensions[0] as f32 / dimensions[1] as f32,
            1.0f32,
            1024.0f32,
        ).into(),
        s_vector: [0.0; 3],
        s_offset: 0.0,
        t_vector: [0.0; 3],
        t_offset: 0.0,
        tex_w: 0.0,
        tex_h: 0.0,
    };

    let pipeline = Arc::new(
        GraphicsPipeline::start()
            .vertex_input_single_buffer::<VkBspVertex>()
            .vertex_shader(vs.main_entry_point(), ())
            .triangle_fan()
            .viewports_dynamic_scissors_irrelevant(1)
            .fragment_shader(fs.main_entry_point(), ())
            .depth_stencil_simple_depth()
            .front_face_clockwise()
            .cull_mode_back()
            .render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
            .build(device.clone())
            .expect("Failed to create graphics pipeline"),
    );

    let mut descriptor_pool = FixedSizeDescriptorSetsPool::new(pipeline.clone(), 0);

    let mut framebuffers: Option<Vec<Arc<Framebuffer<_, _>>>> = None;
    let mut recreate_swapchain = false;
    let mut previous_frame_end = Box::new(now(device.clone())) as Box<GpuFuture>;

    let sampler = Sampler::new(
        device.clone(),
        Filter::Nearest,
        Filter::Nearest,
        MipmapMode::Nearest,
        SamplerAddressMode::Repeat,
        SamplerAddressMode::Repeat,
        SamplerAddressMode::Repeat,
        0.0,
        1.0,
        1.0,
        100.0,
    ).unwrap();

    let bsp = Bsp::load("pak0.pak.d/maps/e1m1.bsp").unwrap();
    let vk_bsp = VkBsp::new(bsp, device.clone(), queue.clone()).unwrap();

    loop {
        previous_frame_end.cleanup_finished();

        if recreate_swapchain {
            dimensions = window
                .surface()
                .capabilities(physical_device)
                .expect("failed to get surface capabilities")
                .current_extent
                .unwrap();

            let (new_swapchain, new_images) = match swapchain.recreate_with_dimension(dimensions) {
                Ok(r) => r,
                Err(vulkano::swapchain::SwapchainCreationError::UnsupportedDimensions) => {
                    continue;
                }
                Err(err) => panic!("{:?}", err),
            };

            std::mem::replace(&mut swapchain, new_swapchain);
            std::mem::replace(&mut images, new_images);

            framebuffers = None;

            recreate_swapchain = false;
        }

        if framebuffers.is_none() {
            let new_framebuffers = Some(
                images
                    .iter()
                    .map(|image| {
                        Arc::new(
                            Framebuffer::start(render_pass.clone())
                                .add(image.clone())
                                .unwrap()
                                .add(depth_buffer.clone())
                                .unwrap()
                                .build()
                                .unwrap(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            std::mem::replace(&mut framebuffers, new_framebuffers);
        }

        let (image_num, acquire_future) =
            match vulkano::swapchain::acquire_next_image(swapchain.clone(), None) {
                Ok(r) => r,
                Err(vulkano::swapchain::AcquireError::OutOfDate) => {
                    recreate_swapchain = true;
                    continue;
                }
                Err(err) => panic!("Error acquiring next image: {:?}", err),
            };

        let mut cmd_buf_builder =
            AutoCommandBufferBuilder::primary_one_time_submit(device.clone(), queue.family())
                .unwrap()
                .begin_render_pass(
                    framebuffers.as_ref().unwrap()[image_num].clone(),
                    false,
                    vec![[0.0, 0.0, 1.0, 1.0].into()],
                )
                .unwrap();

        for face in vk_bsp.faces.iter() {
            let texinfo = &vk_bsp.texinfo[face.texinfo_id];
            let tex_dimensions = &vk_bsp.textures[texinfo.tex_id].img.dimensions();
            pcs.s_vector = texinfo.s_vector;
            pcs.s_offset = texinfo.s_offset;
            pcs.t_vector = texinfo.t_vector;
            pcs.t_offset = texinfo.t_offset;
            pcs.tex_w = tex_dimensions.width() as f32;
            pcs.tex_h = tex_dimensions.height() as f32;
            assert!(&vk_bsp.textures[texinfo.tex_id].img.can_be_sampled(
                &sampler,
            ));

            let descriptor_set = descriptor_pool
                .next()
                .add_sampled_image(vk_bsp.textures[texinfo.tex_id].img.clone(), sampler.clone())
                .unwrap()
                .build()
                .unwrap();

            cmd_buf_builder = cmd_buf_builder
                .draw_indexed(pipeline.clone(),
                              DynamicState {
                                  line_width: None,
                                  // TODO: Find a way to do this without having to dynamically allocate a Vec every frame.
                                  viewports: Some(vec![Viewport {
                                      origin: [0.0, 0.0],
                                      dimensions: [dimensions[0] as f32, dimensions[1] as f32],
                                      depth_range: 0.0 .. 1.0,
                                  }]),
                                  scissors: None,
                              },
                              vk_bsp.vertex_buffer.clone(),
                              face.index_slice.clone(), descriptor_set, pcs)
                .unwrap()
        }

        let command_buffer = cmd_buf_builder.end_render_pass().unwrap().build().unwrap();

        let future = previous_frame_end
            .join(acquire_future)
            .then_execute(queue.clone(), command_buffer)
            .unwrap()
            .then_swapchain_present(queue.clone(), swapchain.clone(), image_num)
            .then_signal_fence_and_flush()
            .unwrap();
        previous_frame_end = Box::new(future) as Box<_>;

        let mut done = false;
        events_loop.poll_events(|event| match event {
            winit::Event::WindowEvent { event: winit::WindowEvent::Closed, .. } => done = true,
            _ => (),
        });

        if done {
            break;
        }
    }

    println!("Using device {}", physical_device.name());
}