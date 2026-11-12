#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User,
    Debug,
}

macro_rules! env_vars {
    ($($variant:ident => $name:literal, $scope:ident, $doc:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum EnvVar {
            $($variant,)*
        }

        impl EnvVar {
            pub const ALL: &'static [EnvVar] = &[$(EnvVar::$variant,)*];

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }

            pub const fn scope(self) -> Scope {
                match self {
                    $(Self::$variant => Scope::$scope,)*
                }
            }

            pub const fn doc(self) -> &'static str {
                match self {
                    $(Self::$variant => $doc,)*
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

env_vars! {
    Std => "VN_STD", User, "carpeta std/ a usar, o `@embedded` para la imagen incrustada";
    StdBundle => "VN_STD_BUNDLE", User, "imagen std.vnb a cargar en lugar de la incrustada";
    Home => "VN_HOME", User, "directorio base de Varn (por defecto ~/.vn)";
    CacheDir => "VN_CACHE_DIR", User, "directorio de cachés (por defecto <home>/cache)";
    NoJit => "VN_NO_JIT", Debug, "bandera: ejecuta solo en el intérprete";
    NoClif => "VN_NO_CLIF", Debug, "bandera: desactiva el backend Cranelift";
    JitCache => "VN_JIT_CACHE", Debug, "caché de código JIT: on, off o verify";
    JitTier => "VN_JIT_TIER", Debug, "llamadas antes de compilar una función con bucles (entero)";
    JitTierStraight => "VN_JIT_TIER_STRAIGHT", Debug, "llamadas antes de compilar una función sin bucles (entero)";
    JitOsr => "VN_JIT_OSR", Debug, "retrocesos de bucle antes de entrar por OSR (entero)";
    ClifTrace => "VN_CLIF_TRACE", Debug, "bandera: traza cada función compilada por Cranelift";
    ClifVerify => "VN_CLIF_VERIFY", Debug, "bandera: verifica el IR de Cranelift (siempre activa en debug)";
    ClifPrintIr => "VN_CLIF_PRINT_IR", Debug, "bandera: imprime el IR de Cranelift";
    ClifOpt => "VN_CLIF_OPT", Debug, "nivel de Cranelift: none, speed o speed_and_size";
    AllocSlow => "VN_ALLOC_SLOW", Debug, "bandera: fuerza la ruta lenta de asignación";
    AllocProfile => "VN_ALLOC_PROFILE", Debug, "nivel del perfilador de asignaciones (0 apagado)";
    HomeTrace => "VN_HOME_TRACE", Debug, "bandera: traza las llamadas nativas con receptor";
    GcTrace => "VN_GC_TRACE", Debug, "bandera: una línea por colección";
    GcVerify => "VN_GC_VERIFY", Debug, "bandera: verifica los invariantes del heap tras cada colección";
    GcStress => "VN_GC_STRESS", Debug, "objetos jóvenes entre colecciones (N pequeño caza raíces perdidas)";
    LspDebug => "VN_LSP_DEBUG", Debug, "bandera: registro detallado del servidor de lenguaje";
    ShadowTarget => "VN_SHADOW_TARGET", Debug, "ejecutable que lanza vn-shadow";
}
