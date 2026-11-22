.PHONY: all build check test pkg abi size bloat lint fmt flash info clean doctor ports dock mount help

all: build

build:
	@./kern build

check:
	@./kern check

test:
	@./kern test

pkg:
	@./kern pkg

abi:
	@./kern abi --check

size:
	@./kern size

bloat:
	@./kern bloat audit

lint:
	@./kern lint report

fmt:
	@./kern fmt

flash:
	@./kern flash

info:
	@./kern info

doctor:
	@./kern doctor

ports:
	@./kern ports

dock:
	@./kern dock

mount:
	@./kern mount

clean:
	@./kern clean

help:
	@./kern help
