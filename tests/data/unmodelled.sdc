create_clock -name clk -period 10 [get_ports clk]
set_disable_timing [get_ports din]
