# Versionado y proceso

## Formato de versión

```text
A-MAJOR.MINOR.PATCH
```

Representación en Cargo: `MAJOR.MINOR.PATCH-alpha`

Ejemplos:

| Tag público | Cargo.toml |
|-------------|------------|
| `A-1.0.0` | `1.0.0-alpha` |
| `A-2.0.0` | `2.0.0-alpha` |

## Entornos

Kroa usa dos GitHub Environments:

| Entorno | Rama / disparador | ¿Publica releases comerciales? |
|---------|-------------------|--------------------------------|
| `development` | pushes y PRs a `develop` | No — solo artifacts temporales de CI |
| `production` | tags `A-*` | Sí — solo GitHub Releases |

```text
feature/* / fix/*
        |
        v
   PR + comprobaciones CI
        |
        v
     develop  ---- artifacts de desarrollo (internos)
        |
   PR de release
        |
        v
       main
        |
   tag anotado A-X.Y.Z
        |
        v
 GitHub Release de producción
```

## Roles de las ramas

- `main`: línea de producción. Solo commits de release verificados.
- `develop`: integración de la siguiente Alpha.
- `feature/*`, `fix/*`: ramas de corta duración. Se fusionan en `develop` por PR.

## Lista de verificación de release

1. Actualizar la versión en `Cargo.toml` / `Cargo.lock`.
2. Actualizar `PROJECT_STATUS.md`, `README.md`, `CHANGELOG.md` y la documentación bilingüe.
3. Abrir un PR de `develop` hacia `main`.
4. Tras el merge, crear un tag anotado: `git tag -a A-X.Y.Z -m "..."`.
5. Empujar el tag. El workflow de producción compila, genera checksums y publica la Release.

## Qué publica producción

- Binarios del compilador para las plataformas soportadas
- Checksums SHA-256
- Notas de release basadas en `CHANGELOG.md` y el mensaje del tag

Las builds de desarrollo nunca deben presentarse como releases de producción.
