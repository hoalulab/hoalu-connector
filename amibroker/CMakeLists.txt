cmake_minimum_required(VERSION 3.20)
project(AmiBrokerMarketDataPlugin LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT MSVC)
    message(FATAL_ERROR "Plugin AmiBroker chi build duoc bang MSVC tren Windows.")
endif()

add_subdirectory(plugin-shim)
