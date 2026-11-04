################################################################################
#
# raofflineproxy
#
################################################################################

RAOFFLINEPROXY_VERSION = 2.3.0-alpha1
RAOFFLINEPROXY_SITE = $(call github,misantronic,RAOfflineProxy,v$(RAOFFLINEPROXY_VERSION))
RAOFFLINEPROXY_LICENSE = GPL-3.0
RAOFFLINEPROXY_LICENSE_FILES = LICENSE
RAOFFLINEPROXY_DEPENDENCIES = python3

# The Linux service only (linux/raofflineproxy): no Windows code, no
# fullscreen menu fonts or donation images (nuubOS has its own settings).
define RAOFFLINEPROXY_INSTALL_TARGET_CMDS
	mkdir -p $(TARGET_DIR)/usr/lib/python$(PYTHON3_VERSION_MAJOR)/site-packages/raofflineproxy
	cp $(@D)/linux/raofflineproxy/*.py \
		$(TARGET_DIR)/usr/lib/python$(PYTHON3_VERSION_MAJOR)/site-packages/raofflineproxy/
endef

$(eval $(generic-package))
