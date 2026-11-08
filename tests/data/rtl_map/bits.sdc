create_clock -name clk -period 10 [get_ports clk]
set_false_path -to [get_pins {r[9]/D}]
set_output_delay 1 -clock clk [get_ports {q[8]}]
