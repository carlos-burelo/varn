#!/usr/bin/env bash
# Script de pre-verificación local de Varn para entornos Unix (Linux / macOS)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
cd "$ROOT_DIR"

FAST=0
SKIP_LINT=0
CLEAN_CACHE=0

for arg in "$@"; do
    case $arg in
        --fast|-f)
            FAST=1
            shift
            ;;
        --skip-lint)
            SKIP_LINT=1
            shift
            ;;
        --clean-cache)
            CLEAN_CACHE=1
            shift
            ;;
    esac
done

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

FAILED_STEPS=()

header() {
    echo ""
    echo -e "${CYAN}======================================================================${NC}"
    echo -e "${CYAN}  >> $1${NC}"
    echo -e "${CYAN}======================================================================${NC}"
}

pass() {
    echo -e "${GREEN} [PASS] $1${NC}"
}

fail() {
    echo -e "${RED} [FAIL] $1${NC}"
}

warn() {
    echo -e "${YELLOW} [WARN] $1${NC}"
}

START_TIME=$(date +%s)

# 1. Formato
if [ $SKIP_LINT -eq 0 ]; then
    header "1/6: Verificación de Formateo (cargo fmt --check)"
    if cargo fmt --all -- --check; then
        pass "Formato de código correcto."
    else
        warn "El código tiene diferencias de formateo con cargo fmt."
    fi

    # 2. Clippy
    header "2/6: Análisis Estático con Clippy (cargo clippy)"
    if cargo clippy --workspace --all-targets; then
        pass "Clippy completado exitosamente."
    else
        fail "Clippy encontró errores de compilación."
        FAILED_STEPS+=("cargo clippy")
    fi
else
    warn "Saltando pasos de formateo y clippy (--skip-lint activo)."
fi

# 3. Gobernanza de Tamaño de Archivos
header "3/6: Auditoría de Gobernanza de Tamaño de Archivos (Anti-God Files)"
LARGE_FILES=0
CRITICAL_FILES=0

while IFS= read -r file; do
    LINES=$(wc -l < "$file")
    if [ "$LINES" -gt 1000 ]; then
        fail "$file tiene $LINES líneas (>1000 líneas - PROHIBIDO)"
        CRITICAL_FILES=$((CRITICAL_FILES + 1))
    elif [ "$LINES" -gt 700 ]; then
        warn "$file tiene $LINES líneas (>700 líneas - Refactor recomendado)"
        LARGE_FILES=$((LARGE_FILES + 1))
    fi
done < <(find crates -name "*.rs")

if [ $CRITICAL_FILES -gt 0 ]; then
    FAILED_STEPS+=("File Size Governance (>1000 lines)")
else
    pass "Todos los archivos de crates cumplen con el límite de 1000 líneas."
fi

# 4. Compilación
header "4/6: Compilación de Producción (cargo build --release --bin vn)"
if cargo build --release --bin vn; then
    pass "Binario 'vn' compilado exitosamente."
else
    fail "Error al compilar el binario 'vn'."
    FAILED_STEPS+=("cargo build --release")
    exit 1
fi

VN_BIN="./target/release/vn"

# Helper cuadrante
run_quadrant() {
    local name="$1"
    local env_flags="$2"
    local cmd_args="$3"

    echo ""
    echo -e "${YELLOW}--- Ejecutando Cuadrante: $name ---${NC}"

    if [ $CLEAN_CACHE -eq 1 ]; then
        $VN_BIN cache clean > /dev/null 2>&1 || true
    fi

    if eval "$env_flags $VN_BIN $cmd_args"; then
        pass "Cuadrante superado: $name"
        return 0
    else
        fail "Cuadrante fallido: $name"
        return 1
    fi
}

# 5. Matriz de Validación de 4 Cuadrantes
header "5/6: Matriz Obligatoria de 4 Cuadrantes (tests/main.vn)"

# Q1: dev-checkout + JIT
if ! run_quadrant "1/4: [dev-checkout] + [JIT Habilitado]" "" "run tests/main.vn"; then
    FAILED_STEPS+=("Cuadrante 1 (dev-checkout + JIT)")
fi

# Q2: dev-checkout + No-JIT
if ! run_quadrant "2/4: [dev-checkout] + [Intérprete Pure (VARN_NO_JIT=1)]" "VARN_NO_JIT=1" "run tests/main.vn"; then
    FAILED_STEPS+=("Cuadrante 2 (dev-checkout + No-JIT)")
fi

# Q3: @embedded + JIT
if ! run_quadrant "3/4: [@embedded std] + [JIT Habilitado]" "VARN_STD=@embedded" "run tests/main.vn"; then
    FAILED_STEPS+=("Cuadrante 3 (@embedded + JIT)")
fi

# Q4: @embedded + No-JIT
if ! run_quadrant "4/4: [@embedded std] + [Intérprete Pure (VARN_NO_JIT=1)]" "VARN_STD=@embedded VARN_NO_JIT=1" "run tests/main.vn"; then
    FAILED_STEPS+=("Cuadrante 4 (@embedded + No-JIT)")
fi

# 6. Benchmark
if [ $FAST -eq 0 ]; then
    header "6/6: Benchmark de Estabilidad (vn bench tests/benchmarks/bench_fib.vn -v)"
    if $VN_BIN bench tests/benchmarks/bench_fib.vn -v; then
        pass "Benchmark de estabilidad completado satisfactoriamente."
    else
        fail "El benchmark de estabilidad reportó errores."
        FAILED_STEPS+=("Benchmark de Estabilidad")
    fi
else
    warn "Saltando paso de benchmark (--fast activo)."
fi

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo ""
echo -e "${CYAN}======================================================================${NC}"
echo -e "${CYAN}  RESUMEN DE PRE-VERIFICACIÓN LOCAL (Tiempo total: ${ELAPSED}s)${NC}"
echo -e "${CYAN}======================================================================${NC}"

if [ ${#FAILED_STEPS[@]} -eq 0 ]; then
    echo -e "\n${GREEN} [TODO CORRECTO] Todas las verificaciones y la matriz de 4 cuadrantes pasaron exitosamente.${NC}"
    echo -e "${GREEN} El código está listo para producción y es seguro hacer commit / push.\n${NC}"
    exit 0
else
    echo -e "\n${RED} [FALLOS DETECTADOS] Los siguientes pasos fallaron:${NC}"
    for step in "${FAILED_STEPS[@]}"; do
        echo -e "${RED}  - $step${NC}"
    done
    echo -e "\n${RED} Por favor corrige los errores anteriores antes de enviar a producción.\n${NC}"
    exit 1
fi
