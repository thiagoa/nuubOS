################################################################################
#
# libretro-flycast
#
################################################################################

# Flycast (Sega Dreamcast / NAOMI): Cortex-A53 GLES profile, arm64 dynarec,
# low-end settings. Pinned upstream commit; part of the default H700 core
# matrix (package/libretro/README.md).
# Bundled as Flycast-nuubOS (flycast_nuubos): patches 0002-0013 are nuubOS
# H700 performance work (docs/dev/emulation.md "Flycast-nuubOS"). Unlike
# the other nuubOS cores there is no upstream download next to it: the
# aarch64 buildbot Flycast needs desktop OpenGL and libgomp.
LIBRETRO_FLYCAST_VERSION = d0fa8b5407251c9c92b44b7c9eb83e3bf84e16c1
LIBRETRO_FLYCAST_SITE = $(call github,libretro,flycast,$(LIBRETRO_FLYCAST_VERSION))
LIBRETRO_FLYCAST_LICENSE = GPL-2.0
LIBRETRO_FLYCAST_LICENSE_FILES = LICENSE
# libretro-core-info: flycast_nuubos_libretro.info is derived from upstream's
LIBRETRO_FLYCAST_DEPENDENCIES = libgles libretro-core-info

define LIBRETRO_FLYCAST_BUILD_CMDS
	$(TARGET_MAKE_ENV) $(LIBRETRO_CORE_ENV) $(MAKE) -C $(@D) -f Makefile \
		platform=arm64_cortex_a53_gles2 HAVE_OPENMP=0 LD="$(TARGET_CXX)" \
		AS="$(TARGET_CC)" CC_AS="$(TARGET_CC)"
endef

define LIBRETRO_FLYCAST_INSTALL_TARGET_CMDS
	$(call LIBRETRO_NUUBOS_CORE_INSTALL,flycast,flycast_nuubos,$(@D)/flycast_libretro.so)
endef

$(eval $(generic-package))
